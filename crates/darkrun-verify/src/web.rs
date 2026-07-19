//! The web backend — drives a real headless Chrome over CDP (via
//! [`chromiumoxide`]), navigates to a URL, collects a [`DomSnapshot`] +
//! [`PageVitals`] with one in-page evaluation, captures a screenshot, and
//! shapes the result into a [`WebProof`](darkrun_api::WebProof).
//!
//! **Browser backend: chromiumoxide (Chrome DevTools Protocol).** It speaks CDP
//! straight to a Chrome/Chromium binary — no Node runtime in the loop — which
//! gives us `performance.*` navigation/paint metrics and `Page.captureScreenshot`
//! for free. The Chrome binary is resolved from `$DARKRUN_CHROME`, then
//! `$CHROME`, then chromiumoxide's own detection.
//!
//! The browser's only job is *collection*. The moment we have a snapshot, the
//! analyzers in [`crate::audit`] take over — pure Rust, no browser — so the
//! audit/proof-shaping logic is exercised in CI without a network or a browser
//! (see the tests in [`crate::audit`] and the load-harness tests).

use std::path::{Path, PathBuf};
use std::time::Duration;

use darkrun_api::{Proof, Surface, WebProof};
use futures::StreamExt;
use serde::Deserialize;

use crate::audit::{audit_snapshot, DomSnapshot, PageVitals};
use crate::error::{Result, VerifyError};

/// The in-page collector script (returns a JSON `{dom, vitals}` string).
const COLLECTOR_JS: &str = include_str!("collector.js");

/// Options for a web capture.
#[derive(Debug, Clone)]
pub struct WebOpts {
    /// Where to write the captured screenshot PNG. When `None`, no screenshot
    /// file is written (the proof still carries vitals + audits).
    pub screenshot_path: Option<PathBuf>,
    /// How long to wait after navigation for paint/layout metrics to settle.
    pub settle: Duration,
    /// Viewport width in CSS pixels.
    pub width: u32,
    /// Viewport height in CSS pixels.
    pub height: u32,
    /// Overall ceiling on the whole capture — if Chrome hangs, we bail.
    pub timeout: Duration,
}

impl Default for WebOpts {
    fn default() -> Self {
        WebOpts {
            screenshot_path: None,
            settle: Duration::from_millis(800),
            width: 1280,
            height: 800,
            timeout: Duration::from_secs(30),
        }
    }
}

/// The raw `{dom, vitals, reach}` payload the collector script returns.
#[derive(Debug, Deserialize)]
struct Collected {
    dom: DomSnapshot,
    vitals: PageVitals,
    /// Reachability signals, absent on older collector payloads, so defaulted
    /// (a missing block reads as "reachable", the pre-existing behavior).
    #[serde(default)]
    reach: Reach,
}

/// Whether the browser actually reached the target, or landed on an error page.
/// Collected by `collector.js`; the verdict is decided by the pure
/// [`unreachable_reason`] so it is testable without a browser.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Reach {
    /// Chrome rendered its own network-error interstitial (`#main-frame-error`),
    /// meaning the server was never reached (DNS failure, connection refused).
    #[serde(default)]
    pub chrome_error_page: bool,
    /// The main document's HTTP status. `0` for `file://` / `data:` / a failed
    /// net request; `>= 400` is an error page the target served.
    #[serde(default)]
    pub http_status: u16,
}

/// Decide whether a capture landed on an error page rather than the real target.
/// Returns `Some(reason)` when the proof must be REFUSED (the target was
/// unreachable or served an HTTP error), or `None` when the page is real. Pure,
/// so the honesty rule is unit-tested without a browser.
///
/// A `file://` / `data:` load reports status `0` and no interstitial, so it
/// passes; a `2xx`/`3xx` page passes; a Chrome net-error interstitial or a
/// `>= 400` response fails.
pub fn unreachable_reason(reach: &Reach) -> Option<String> {
    if reach.chrome_error_page {
        Some("chrome network-error page (target unreachable)".to_string())
    } else if reach.http_status >= 400 {
        Some(format!("target returned HTTP {}", reach.http_status))
    } else {
        None
    }
}

/// Shape a collected snapshot + vitals + optional screenshot URL into a
/// [`WebProof`]. **Pure** — no browser, no IO — so the mapping from raw
/// measurement to proof is unit-tested directly.
pub fn shape_web_proof(
    snap: &DomSnapshot,
    vitals: &PageVitals,
    screenshot_url: Option<String>,
) -> WebProof {
    WebProof {
        vitals: vitals.to_map(),
        audits: audit_snapshot(snap),
        screenshot_url,
    }
}

/// Wrap a [`WebProof`] in a surface-tagged [`Proof`]. The surface defaults to
/// [`Surface::WebUi`] but desktop/mobile share the same browser route.
pub fn web_proof_into(proof: WebProof, surface: Surface) -> Proof {
    Proof::web(surface, proof)
}

/// Validate a capture target up front so we fail with a clear message rather
/// than deep inside the browser. Accepts `http(s)://`, `file://`, and
/// `data:` targets.
pub fn validate_target(url: &str) -> Result<()> {
    let u = url.trim();
    if u.starts_with("http://")
        || u.starts_with("https://")
        || u.starts_with("file://")
        || u.starts_with("data:")
    {
        Ok(())
    } else {
        Err(VerifyError::Target(format!(
            "{url:?} — expected an http(s)://, file://, or data: URL"
        )))
    }
}

/// Drive headless Chrome to capture a [`WebProof`] for `url`.
///
/// Returns the proof; if `opts.screenshot_path` is set, the PNG is written
/// there and its path becomes the proof's `screenshot_url`.
/// Excluded from coverage: drives a real headless browser (CDP) — no test process.
#[cfg(not(tarpaulin_include))]
pub async fn verify_web(url: &str, opts: &WebOpts) -> Result<WebProof> {
    validate_target(url)?;
    // The whole capture is bounded by `opts.timeout` so a wedged browser can't
    // hang the Prove station.
    tokio::time::timeout(opts.timeout, capture(url, opts))
        .await
        .map_err(|_| VerifyError::Browser(format!("capture timed out after {:?}", opts.timeout)))?
}

/// Resolve the Chrome executable, honoring env overrides.
fn chrome_path() -> Option<PathBuf> {
    for var in ["DARKRUN_CHROME", "CHROME"] {
        if let Ok(p) = std::env::var(var) {
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
    }
    None
}

/// The actual CDP capture (wrapped in a timeout by [`verify_web`]).
#[cfg(not(tarpaulin_include))]
async fn capture(url: &str, opts: &WebOpts) -> Result<WebProof> {
    use chromiumoxide::browser::{Browser, BrowserConfig};

    let mut builder = BrowserConfig::builder()
        .window_size(opts.width, opts.height)
        .viewport(None);
    if let Some(chrome) = chrome_path() {
        builder = builder.chrome_executable(chrome);
    }
    let config = builder
        .build()
        .map_err(|e| VerifyError::Browser(e.to_string()))?;

    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| VerifyError::Browser(e.to_string()))?;
    // Drive the CDP event loop on a background task for the lifetime of the
    // capture.
    let handle = tokio::spawn(async move { while handler.next().await.is_some() {} });

    let result = capture_on_browser(&browser, url, opts).await;

    // Always tear the browser down, even on error.
    let _ = browser.close().await;
    let _ = handle.await;
    result
}

/// The page-level capture, factored out so the browser teardown in [`capture`]
/// runs on every path.
#[cfg(not(tarpaulin_include))]
async fn capture_on_browser(
    browser: &chromiumoxide::Browser,
    url: &str,
    opts: &WebOpts,
) -> Result<WebProof> {
    let page = browser
        .new_page(url)
        .await
        .map_err(|e| VerifyError::Browser(e.to_string()))?;
    page.wait_for_navigation()
        .await
        .map_err(|e| VerifyError::Browser(e.to_string()))?;

    // Give paint/layout-shift observers a beat to record.
    tokio::time::sleep(opts.settle).await;

    let raw = page
        .evaluate(COLLECTOR_JS)
        .await
        .map_err(|e| VerifyError::Metrics(e.to_string()))?;
    let json: String = raw
        .into_value()
        .map_err(|e| VerifyError::Metrics(format!("collector returned non-string: {e}")))?;
    let collected: Collected = serde_json::from_str(&json)?;

    // HONESTY GUARD: if the browser landed on Chrome's own error page or an HTTP
    // error response, there is no real page to prove, so fail loudly (no proof,
    // no screenshot) instead of certifying the error page's DOM as the target.
    if let Some(reason) = unreachable_reason(&collected.reach) {
        return Err(VerifyError::Unreachable(format!("{url}: {reason}")));
    }

    // Capture a screenshot if requested.
    let screenshot_url = match &opts.screenshot_path {
        Some(path) => {
            let bytes = page
                .screenshot(
                    chromiumoxide::page::ScreenshotParams::builder()
                        .full_page(false)
                        .build(),
                )
                .await
                .map_err(|e| VerifyError::Browser(e.to_string()))?;
            write_screenshot(path, &bytes)?;
            Some(path.display().to_string())
        }
        None => None,
    };

    Ok(shape_web_proof(&collected.dom, &collected.vitals, screenshot_url))
}

/// Persist a screenshot's PNG bytes, creating parent dirs as needed.
fn write_screenshot(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{ContrastSample, ImageInfo, TouchTarget};

    #[test]
    fn validate_target_accepts_supported_schemes() {
        assert!(validate_target("http://localhost:8080").is_ok());
        assert!(validate_target("https://example.com/page").is_ok());
        assert!(validate_target("file:///tmp/index.html").is_ok());
        assert!(validate_target("data:text/html,<h1>hi</h1>").is_ok());
    }

    #[test]
    fn validate_target_rejects_bare_paths_and_unknown_schemes() {
        assert!(validate_target("example.com").is_err());
        assert!(validate_target("/tmp/index.html").is_err());
        assert!(validate_target("ftp://host/x").is_err());
    }

    fn snap() -> DomSnapshot {
        DomSnapshot {
            text_contrasts: vec![ContrastSample { label: "p".into(), ratio: 8.0 }],
            touch_targets: vec![TouchTarget { label: "button".into(), width: 50.0, height: 50.0 }],
            images: vec![ImageInfo { label: "x.png".into(), has_alt: true }],
            honors_reduced_motion: true,
            landmark_count: 2,
            has_main_landmark: true,
            keyboard_focusable: 1,
            interactive_total: 1,
            has_document_title: true,
            has_lang: true,
        }
    }

    #[test]
    fn shape_web_proof_folds_vitals_and_audits() {
        let vitals = PageVitals {
            lcp: Some(900.0),
            fcp: Some(400.0),
            cls: Some(0.0),
            ttfb: Some(50.0),
            ..Default::default()
        };
        let proof = shape_web_proof(&snap(), &vitals, Some("/shot.png".into()));
        assert_eq!(proof.vitals.get("lcp"), Some(&900.0));
        assert_eq!(proof.vitals.get("ttfb"), Some(&50.0));
        assert_eq!(proof.audits.len(), 8);
        assert!(proof.all_audits_pass(), "clean snapshot passes");
        assert_eq!(proof.screenshot_url.as_deref(), Some("/shot.png"));
    }

    #[test]
    fn shape_web_proof_surfaces_failures() {
        let mut s = snap();
        s.touch_targets.push(TouchTarget { label: "tiny".into(), width: 10.0, height: 10.0 });
        let proof = shape_web_proof(&s, &PageVitals::default(), None);
        assert!(!proof.all_audits_pass());
        assert!(proof.screenshot_url.is_none());
    }

    #[test]
    fn web_proof_into_tags_surface_and_matches_route() {
        let proof = web_proof_into(shape_web_proof(&snap(), &PageVitals::default(), None), Surface::Desktop);
        assert_eq!(proof.surface, Surface::Desktop);
        assert!(proof.block_matches_surface(), "web block matches a visual surface");
        assert!(proof.web.is_some());
        assert!(proof.bench.is_none());
    }

    #[test]
    fn collected_payload_deserializes_from_collector_shape() {
        // Mirrors what collector.js returns — proves the wire contract holds.
        let json = r#"{
            "dom": {"text_contrasts":[{"label":"p","ratio":9.1}],"touch_targets":[],
                    "images":[],"honors_reduced_motion":false,"landmark_count":1,
                    "has_main_landmark":true,"keyboard_focusable":0,"interactive_total":0,
                    "has_document_title":true,"has_lang":true},
            "vitals": {"ttfb":40,"fcp":300,"lcp":700,"cls":0.01,"inp":null,
                       "transfer_size":12000,"js_heap_used":null}
        }"#;
        let c: Collected = serde_json::from_str(json).unwrap();
        assert_eq!(c.dom.text_contrasts[0].ratio, 9.1);
        assert_eq!(c.vitals.lcp, Some(700.0));
        let proof = shape_web_proof(&c.dom, &c.vitals, None);
        assert_eq!(proof.vitals.get("transfer_size"), Some(&12000.0));
    }

    #[test]
    fn unreachable_reason_flags_error_pages() {
        // Chrome's own network-error interstitial → unreachable.
        let r = unreachable_reason(&Reach { chrome_error_page: true, http_status: 0 });
        assert!(r.as_deref().unwrap().contains("unreachable"), "got {r:?}");
        // A 4xx/5xx the target served → refused, with the status surfaced.
        assert_eq!(
            unreachable_reason(&Reach { chrome_error_page: false, http_status: 404 }).as_deref(),
            Some("target returned HTTP 404")
        );
        assert!(unreachable_reason(&Reach { chrome_error_page: false, http_status: 503 }).is_some());
    }

    #[test]
    fn unreachable_reason_passes_real_pages_and_local_schemes() {
        // A healthy 200 page is reachable.
        assert!(unreachable_reason(&Reach { chrome_error_page: false, http_status: 200 }).is_none());
        // A redirect that resolved is reachable.
        assert!(unreachable_reason(&Reach { chrome_error_page: false, http_status: 302 }).is_none());
        // file:// / data: report status 0 with no interstitial, so must not fail.
        assert!(unreachable_reason(&Reach::default()).is_none());
    }

    #[test]
    fn collected_defaults_reach_when_the_payload_omits_it() {
        // Older collector payloads carried no `reach` block; it must default to a
        // reachable verdict so their proofs still shape.
        let json = r#"{
            "dom": {"text_contrasts":[],"touch_targets":[],"images":[],
                    "honors_reduced_motion":false,"landmark_count":0,
                    "has_main_landmark":true,"keyboard_focusable":0,"interactive_total":0,
                    "has_document_title":true,"has_lang":true},
            "vitals": {}
        }"#;
        let c: Collected = serde_json::from_str(json).unwrap();
        assert_eq!(c.reach, Reach::default());
        assert!(unreachable_reason(&c.reach).is_none());
    }

    #[test]
    fn collected_reads_the_reach_block_when_present() {
        let json = r#"{
            "dom": {"text_contrasts":[],"touch_targets":[],"images":[],
                    "honors_reduced_motion":false,"landmark_count":0,
                    "has_main_landmark":false,"keyboard_focusable":0,"interactive_total":0,
                    "has_document_title":false,"has_lang":false},
            "vitals": {},
            "reach": {"chrome_error_page": true, "http_status": 0}
        }"#;
        let c: Collected = serde_json::from_str(json).unwrap();
        assert!(c.reach.chrome_error_page);
        assert!(unreachable_reason(&c.reach).is_some(), "an error page is refused");
    }

    #[test]
    fn write_screenshot_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deep/shot.png");
        write_screenshot(&path, b"\x89PNG fake").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"\x89PNG fake");
    }

    #[test]
    fn shape_web_proof_and_into_map_cleanly() {
        let snap = crate::audit::DomSnapshot::default();
        let vitals = crate::audit::PageVitals::default();
        let proof = shape_web_proof(&snap, &vitals, Some("/shot.png".into()));
        assert_eq!(proof.screenshot_url.as_deref(), Some("/shot.png"));
        let _ = web_proof_into(proof, darkrun_api::proof::Surface::WebUi);
    }

    #[test]
    fn chrome_path_honors_env_overrides() {
        std::env::set_var("DARKRUN_CHROME", "/opt/chrome");
        assert_eq!(chrome_path(), Some(std::path::PathBuf::from("/opt/chrome")));
        std::env::remove_var("DARKRUN_CHROME");
        std::env::remove_var("CHROME");
        let _ = chrome_path();
    }

    #[test]
    fn web_opts_default_has_sensible_values() {
        let o = WebOpts::default();
        assert!(o.screenshot_path.is_none());
        assert_eq!(o.width, 1280);
        assert_eq!(o.height, 800);
        assert_eq!(o.settle, Duration::from_millis(800));
        assert_eq!(o.timeout, Duration::from_secs(30));
    }
}
