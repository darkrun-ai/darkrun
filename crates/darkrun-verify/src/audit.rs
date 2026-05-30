//! The objective analyzers — pure functions over a [`DomSnapshot`] and
//! [`PageVitals`] that produce the [`AuditResult`]s and vitals map of a
//! [`WebProof`](darkrun_api::WebProof).
//!
//! The headless browser's only job is to *collect* a snapshot (one JS
//! evaluation over the live DOM) and the navigation/paint vitals. Everything
//! that turns that raw data into a pass/fail audit lives here, as plain Rust
//! over serializable inputs — so the audit logic is fully testable from a
//! static HTML fixture with no browser and no network in the loop.

use std::collections::BTreeMap;

use darkrun_api::AuditResult;
use serde::{Deserialize, Serialize};

/// The minimum WCAG AA contrast ratio for normal-size body text.
pub const MIN_CONTRAST_AA: f64 = 4.5;
/// The minimum recommended touch-target edge, in CSS pixels (WCAG 2.5.5).
pub const MIN_TOUCH_TARGET_PX: f64 = 44.0;

/// A live-DOM snapshot collected by the headless browser via a single JS
/// evaluation. Every field is something the analyzers reason over; the browser
/// driver fills it and the analyzers never touch the browser.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DomSnapshot {
    /// One entry per text node whose computed color/background was measured.
    #[serde(default)]
    pub text_contrasts: Vec<ContrastSample>,
    /// One entry per interactive control (button / link / input) with its
    /// rendered box size in CSS pixels.
    #[serde(default)]
    pub touch_targets: Vec<TouchTarget>,
    /// One entry per `<img>` and its alt-text presence.
    #[serde(default)]
    pub images: Vec<ImageInfo>,
    /// Whether the page declares a `prefers-reduced-motion` media handler in
    /// any stylesheet (objective signal that motion is gated on the OS pref).
    #[serde(default)]
    pub honors_reduced_motion: bool,
    /// Count of ARIA/HTML landmark regions (`main`, `nav`, `header`, `footer`,
    /// `[role=...]`) — a basic landmark-structure signal.
    #[serde(default)]
    pub landmark_count: u32,
    /// Whether a single `<main>` (or `[role=main]`) landmark is present.
    #[serde(default)]
    pub has_main_landmark: bool,
    /// Count of focusable controls that are reachable by keyboard (not
    /// `tabindex=-1` and not `disabled`).
    #[serde(default)]
    pub keyboard_focusable: u32,
    /// Count of interactive controls overall (focusable or not).
    #[serde(default)]
    pub interactive_total: u32,
    /// Whether the document declares a non-empty `<title>`.
    #[serde(default)]
    pub has_document_title: bool,
    /// Whether the document declares a `lang` attribute on `<html>`.
    #[serde(default)]
    pub has_lang: bool,
}

/// One measured text/background contrast pair.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContrastSample {
    /// A short locator for the offending node (tag + truncated text), for the
    /// audit value when it fails.
    #[serde(default)]
    pub label: String,
    /// The measured contrast ratio (1.0..=21.0).
    pub ratio: f64,
}

/// One interactive control's rendered size.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TouchTarget {
    /// A short locator for the control.
    #[serde(default)]
    pub label: String,
    /// Rendered width in CSS pixels.
    pub width: f64,
    /// Rendered height in CSS pixels.
    pub height: f64,
}

impl TouchTarget {
    /// The smaller of the two edges — the dimension the 44px rule applies to.
    pub fn min_edge(&self) -> f64 {
        self.width.min(self.height)
    }
}

/// One image's alt-text presence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageInfo {
    /// A short locator (the `src`, truncated).
    #[serde(default)]
    pub label: String,
    /// Whether the image carries a non-empty `alt` (or `role="presentation"`).
    pub has_alt: bool,
}

/// Raw page vitals collected from the browser's Performance API. Kept separate
/// from the snapshot because they come from `performance.*` rather than the DOM.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PageVitals {
    /// Time to first byte (ms) — `responseStart - requestStart`.
    #[serde(default)]
    pub ttfb: Option<f64>,
    /// First contentful paint (ms).
    #[serde(default)]
    pub fcp: Option<f64>,
    /// Largest contentful paint (ms).
    #[serde(default)]
    pub lcp: Option<f64>,
    /// Cumulative layout shift (unitless).
    #[serde(default)]
    pub cls: Option<f64>,
    /// Interaction to next paint (ms), if any interaction was observed.
    #[serde(default)]
    pub inp: Option<f64>,
    /// Total transfer size of the navigation (bytes).
    #[serde(default)]
    pub transfer_size: Option<f64>,
    /// JS heap used (bytes), if the browser exposes it.
    #[serde(default)]
    pub js_heap_used: Option<f64>,
}

impl PageVitals {
    /// Fold the populated vitals into the `BTreeMap` shape a
    /// [`WebProof`](darkrun_api::WebProof) carries. Only present metrics are
    /// inserted, and the spec's core five (`lcp/fcp/cls/ttfb/inp`) are keyed by
    /// their canonical names.
    pub fn to_map(&self) -> BTreeMap<String, f64> {
        let mut m = BTreeMap::new();
        let mut put = |k: &str, v: Option<f64>| {
            if let Some(v) = v {
                m.insert(k.to_string(), v);
            }
        };
        put("lcp", self.lcp);
        put("fcp", self.fcp);
        put("cls", self.cls);
        put("ttfb", self.ttfb);
        put("inp", self.inp);
        put("transfer_size", self.transfer_size);
        put("js_heap_used", self.js_heap_used);
        m
    }
}

/// Run the full audit suite over a snapshot, producing the ordered
/// [`AuditResult`] list a [`WebProof`](darkrun_api::WebProof) carries.
///
/// Each audit is objective: a measured value plus a threshold comparison. The
/// ordering is stable so proofs diff cleanly between runs.
pub fn audit_snapshot(snap: &DomSnapshot) -> Vec<AuditResult> {
    vec![
        contrast_audit(snap),
        touch_target_audit(snap),
        image_alt_audit(snap),
        reduced_motion_audit(snap),
        landmark_audit(snap),
        keyboard_audit(snap),
        document_title_audit(snap),
        lang_audit(snap),
    ]
}

/// Contrast audit — passes when every measured text node clears WCAG AA (4.5:1).
/// The value reports the worst observed ratio.
pub fn contrast_audit(snap: &DomSnapshot) -> AuditResult {
    let worst = snap
        .text_contrasts
        .iter()
        .map(|c| c.ratio)
        .fold(f64::INFINITY, f64::min);
    if snap.text_contrasts.is_empty() {
        return AuditResult {
            name: "contrast".into(),
            value: "n/a".into(),
            pass: true,
        };
    }
    AuditResult {
        name: "contrast".into(),
        value: format!("{worst:.2}:1"),
        pass: worst >= MIN_CONTRAST_AA,
    }
}

/// Touch-target audit — passes when every interactive control is at least
/// 44x44 CSS px. The value reports how many controls were undersized.
pub fn touch_target_audit(snap: &DomSnapshot) -> AuditResult {
    let undersized = snap
        .touch_targets
        .iter()
        .filter(|t| t.min_edge() < MIN_TOUCH_TARGET_PX)
        .count();
    let value = if undersized == 0 {
        format!("{} ok", snap.touch_targets.len())
    } else {
        let smallest = snap
            .touch_targets
            .iter()
            .map(TouchTarget::min_edge)
            .fold(f64::INFINITY, f64::min);
        format!("{undersized} under 44px (min {smallest:.0}px)")
    };
    AuditResult {
        name: "touch-target".into(),
        value,
        pass: undersized == 0,
    }
}

/// Image-alt audit — passes when every `<img>` carries alt text.
pub fn image_alt_audit(snap: &DomSnapshot) -> AuditResult {
    let missing = snap.images.iter().filter(|i| !i.has_alt).count();
    AuditResult {
        name: "image-alt".into(),
        value: if missing == 0 {
            format!("{} ok", snap.images.len())
        } else {
            format!("{missing} missing alt")
        },
        pass: missing == 0,
    }
}

/// Reduced-motion audit — passes when the page gates motion on
/// `prefers-reduced-motion`.
pub fn reduced_motion_audit(snap: &DomSnapshot) -> AuditResult {
    AuditResult {
        name: "reduced-motion".into(),
        value: if snap.honors_reduced_motion {
            "honored".into()
        } else {
            "not handled".into()
        },
        pass: snap.honors_reduced_motion,
    }
}

/// Landmark audit — passes when a `<main>` landmark exists (the minimum
/// structure a screen reader needs to skip to content).
pub fn landmark_audit(snap: &DomSnapshot) -> AuditResult {
    AuditResult {
        name: "landmarks".into(),
        value: format!(
            "{} region(s){}",
            snap.landmark_count,
            if snap.has_main_landmark { "" } else { ", no main" }
        ),
        pass: snap.has_main_landmark,
    }
}

/// Keyboard-reachability audit — passes when every interactive control is
/// keyboard focusable.
pub fn keyboard_audit(snap: &DomSnapshot) -> AuditResult {
    let unreachable = snap.interactive_total.saturating_sub(snap.keyboard_focusable);
    AuditResult {
        name: "keyboard".into(),
        value: if unreachable == 0 {
            format!("{} reachable", snap.keyboard_focusable)
        } else {
            format!("{unreachable} unreachable")
        },
        pass: unreachable == 0,
    }
}

/// Document-title audit — passes when the page declares a non-empty `<title>`.
pub fn document_title_audit(snap: &DomSnapshot) -> AuditResult {
    AuditResult {
        name: "document-title".into(),
        value: if snap.has_document_title {
            "present".into()
        } else {
            "missing".into()
        },
        pass: snap.has_document_title,
    }
}

/// Language audit — passes when `<html>` declares a `lang`.
pub fn lang_audit(snap: &DomSnapshot) -> AuditResult {
    AuditResult {
        name: "html-lang".into(),
        value: if snap.has_lang { "set".into() } else { "missing".into() },
        pass: snap.has_lang,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean_snapshot() -> DomSnapshot {
        DomSnapshot {
            text_contrasts: vec![
                ContrastSample { label: "h1".into(), ratio: 12.6 },
                ContrastSample { label: "p".into(), ratio: 7.1 },
            ],
            touch_targets: vec![
                TouchTarget { label: "button".into(), width: 48.0, height: 48.0 },
                TouchTarget { label: "a".into(), width: 120.0, height: 44.0 },
            ],
            images: vec![ImageInfo { label: "logo.png".into(), has_alt: true }],
            honors_reduced_motion: true,
            landmark_count: 3,
            has_main_landmark: true,
            keyboard_focusable: 2,
            interactive_total: 2,
            has_document_title: true,
            has_lang: true,
        }
    }

    #[test]
    fn clean_page_passes_every_audit() {
        let results = audit_snapshot(&clean_snapshot());
        assert_eq!(results.len(), 8);
        for r in &results {
            assert!(r.pass, "audit {} unexpectedly failed: {}", r.name, r.value);
        }
    }

    #[test]
    fn contrast_fails_on_low_ratio() {
        let mut snap = clean_snapshot();
        snap.text_contrasts.push(ContrastSample { label: "faint".into(), ratio: 2.3 });
        let r = contrast_audit(&snap);
        assert!(!r.pass);
        assert_eq!(r.value, "2.30:1");
    }

    #[test]
    fn contrast_na_when_no_text_measured() {
        let snap = DomSnapshot::default();
        let r = contrast_audit(&snap);
        assert!(r.pass, "no text is vacuously passing");
        assert_eq!(r.value, "n/a");
    }

    #[test]
    fn touch_target_flags_undersized_and_reports_smallest() {
        let mut snap = clean_snapshot();
        snap.touch_targets.push(TouchTarget { label: "x".into(), width: 30.0, height: 30.0 });
        snap.touch_targets.push(TouchTarget { label: "y".into(), width: 100.0, height: 20.0 });
        let r = touch_target_audit(&snap);
        assert!(!r.pass);
        assert!(r.value.contains("2 under 44px"), "got {}", r.value);
        assert!(r.value.contains("min 20px"), "got {}", r.value);
    }

    #[test]
    fn min_edge_is_the_smaller_dimension() {
        let t = TouchTarget { label: "wide".into(), width: 200.0, height: 10.0 };
        assert_eq!(t.min_edge(), 10.0);
    }

    #[test]
    fn image_alt_fails_when_missing() {
        let mut snap = clean_snapshot();
        snap.images.push(ImageInfo { label: "hero.jpg".into(), has_alt: false });
        let r = image_alt_audit(&snap);
        assert!(!r.pass);
        assert_eq!(r.value, "1 missing alt");
    }

    #[test]
    fn reduced_motion_fails_when_unhandled() {
        let mut snap = clean_snapshot();
        snap.honors_reduced_motion = false;
        let r = reduced_motion_audit(&snap);
        assert!(!r.pass);
        assert_eq!(r.value, "not handled");
    }

    #[test]
    fn landmark_requires_main() {
        let mut snap = clean_snapshot();
        snap.has_main_landmark = false;
        let r = landmark_audit(&snap);
        assert!(!r.pass);
        assert!(r.value.contains("no main"), "got {}", r.value);
    }

    #[test]
    fn keyboard_flags_unreachable_controls() {
        let mut snap = clean_snapshot();
        snap.interactive_total = 5;
        snap.keyboard_focusable = 3;
        let r = keyboard_audit(&snap);
        assert!(!r.pass);
        assert_eq!(r.value, "2 unreachable");
    }

    #[test]
    fn document_title_and_lang_audits() {
        let mut snap = clean_snapshot();
        snap.has_document_title = false;
        snap.has_lang = false;
        assert!(!document_title_audit(&snap).pass);
        assert!(!lang_audit(&snap).pass);
    }

    #[test]
    fn vitals_map_keeps_only_present_metrics() {
        let v = PageVitals {
            ttfb: Some(80.0),
            fcp: Some(420.0),
            lcp: Some(900.0),
            cls: Some(0.01),
            inp: None,
            transfer_size: Some(15_000.0),
            js_heap_used: None,
        };
        let m = v.to_map();
        assert_eq!(m.get("ttfb"), Some(&80.0));
        assert_eq!(m.get("lcp"), Some(&900.0));
        assert_eq!(m.get("cls"), Some(&0.01));
        assert_eq!(m.get("transfer_size"), Some(&15_000.0));
        assert!(!m.contains_key("inp"), "absent metric is omitted");
        assert!(!m.contains_key("js_heap_used"));
    }

    #[test]
    fn snapshot_roundtrips_through_json() {
        // The browser hands us this exact JSON shape; it must deserialize back.
        let snap = clean_snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        let back: DomSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap, back);
    }

    #[test]
    fn snapshot_tolerates_missing_fields() {
        // A minimal JS payload (only contrasts) must still parse — every other
        // field is `#[serde(default)]`.
        let json = r#"{"text_contrasts":[{"label":"p","ratio":9.0}]}"#;
        let snap: DomSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snap.text_contrasts.len(), 1);
        assert!(!snap.has_main_landmark);
        assert_eq!(snap.landmark_count, 0);
    }
}
