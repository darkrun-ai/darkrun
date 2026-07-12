//! Objective-evidence payloads — the Prove station's NUMBERS.
//!
//! darkrun's verification is **objective measurement**, not an agent reading
//! code and asserting quality. A run's [`Surface`] (classified at the Shape
//! station) routes which measurement applies:
//!
//! - [`Surface::WebUi`] / [`Surface::Desktop`] / [`Surface::Mobile`] — a real
//!   headless browser: a [`WebProof`] carries web vitals (LCP/FCP/CLS/TTFB/INP)
//!   plus a11y/contrast/touch-target/reduced-motion [`AuditResult`]s and a
//!   screenshot.
//! - [`Surface::Library`] / [`Surface::Api`] / [`Surface::Data`] — criterion
//!   microbenchmarks + a small load harness: a [`BenchProof`] carries latency
//!   percentiles + throughput.
//! - [`Surface::Tui`] / [`Surface::Cli`] — a terminal/output snapshot (carried
//!   as a [`WebProof`]-free [`Proof`] with just a screenshot).
//!
//! A [`Proof`] is the run's attached objective evidence — surface-tagged, with
//! the relevant measurement block populated.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The kind of SURFACE a run delivers — mirrors
/// [`darkrun_core::domain::Surface`] on the wire, kept local so `darkrun-api`
/// stays dependency-light.
///
/// The surface is the linchpin that routes objective verification: visual
/// surfaces go through a headless browser, bench surfaces through criterion +
/// a load harness, terminal surfaces through an output snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    /// A reusable code library (criterion benches + load harness).
    Library,
    /// A network API surface (criterion benches + load harness).
    Api,
    /// A web UI (headless browser: screenshot + vitals + a11y audits).
    WebUi,
    /// A terminal UI (terminal snapshot + interaction).
    Tui,
    /// A command-line tool (output snapshot + interaction).
    Cli,
    /// A desktop application (headless browser: screenshot + vitals + a11y).
    Desktop,
    /// A mobile application (headless browser: screenshot + vitals + a11y).
    Mobile,
    /// A data pipeline / dataset (criterion benches + load harness).
    Data,
}

impl Surface {
    /// The serde token for this surface.
    pub fn as_str(self) -> &'static str {
        match self {
            Surface::Library => "library",
            Surface::Api => "api",
            Surface::WebUi => "web_ui",
            Surface::Tui => "tui",
            Surface::Cli => "cli",
            Surface::Desktop => "desktop",
            Surface::Mobile => "mobile",
            Surface::Data => "data",
        }
    }

    /// Whether this surface is verified through a real headless browser
    /// (a [`WebProof`] block).
    pub fn is_visual(self) -> bool {
        matches!(self, Surface::WebUi | Surface::Desktop | Surface::Mobile)
    }

    /// Whether this surface is verified through criterion + a load harness
    /// (a [`BenchProof`] block).
    pub fn is_bench(self) -> bool {
        matches!(self, Surface::Library | Surface::Api | Surface::Data)
    }

    /// Whether this surface is verified through a terminal/output snapshot.
    pub fn is_terminal(self) -> bool {
        matches!(self, Surface::Tui | Surface::Cli)
    }
}

/// One objective audit result — a named check with its measured value and a
/// boolean pass/fail. Audits are the a11y/contrast/touch-target/reduced-motion
/// checks the headless browser runs against a visual surface.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuditResult {
    /// The audit name (e.g. `contrast`, `touch-target`, `reduced-motion`).
    pub name: String,
    /// The measured value, rendered as a string (e.g. `4.8:1`, `44px`, `0`).
    pub value: String,
    /// Whether the audit passed its threshold.
    pub pass: bool,
}

/// The web-vitals + audit block of a [`Proof`] — populated for visual surfaces
/// (web-ui / desktop / mobile) measured through a real headless browser.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct WebProof {
    /// Web vitals keyed by metric name (`lcp`, `fcp`, `cls`, `ttfb`, `inp`),
    /// in their native units (ms, or unitless for CLS).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vitals: BTreeMap<String, f64>,
    /// The objective audits (a11y/contrast/touch-target/reduced-motion).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audits: Vec<AuditResult>,
    /// URL of the captured screenshot the proof was measured against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_url: Option<String>,
}

impl WebProof {
    /// Whether every audit in the block passed (vacuously true with no audits).
    pub fn all_audits_pass(&self) -> bool {
        self.audits.iter().all(|a| a.pass)
    }

    /// Whether this block carries any real measurement — at least one captured
    /// web vital or a11y audit. A bare/`default()` block (no vitals, no audits)
    /// is NOT a measurement: it is a placeholder that must not count as evidence.
    /// A lone screenshot is deliberately excluded (it proves a snapshot exists,
    /// not that the vitals/audits were run) — it satisfies only the terminal
    /// snapshot route, handled in [`Proof::is_evidence`].
    pub fn has_measurement(&self) -> bool {
        !self.vitals.is_empty() || !self.audits.is_empty()
    }
}

/// The benchmark block of a [`Proof`] — populated for bench surfaces
/// (library / api / data) measured through criterion + a small load harness.
///
/// Percentiles are latency in milliseconds; throughput is operations per
/// second.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct BenchProof {
    /// 50th-percentile latency (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p50: Option<f64>,
    /// 95th-percentile latency (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p95: Option<f64>,
    /// 99th-percentile latency (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p99: Option<f64>,
    /// Throughput (operations per second).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput: Option<f64>,
    /// Number of samples the percentiles were computed over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub samples: Option<u64>,
}

impl BenchProof {
    /// Whether this block carries any real measurement — at least one populated
    /// numeric field (a percentile, throughput, or sample count). An all-`None`
    /// (`default()`) block is a placeholder, not evidence.
    pub fn has_measurement(&self) -> bool {
        self.p50.is_some()
            || self.p95.is_some()
            || self.p99.is_some()
            || self.throughput.is_some()
            || self.samples.is_some()
    }
}

/// The PROOF payload — a run's attached objective evidence.
///
/// Surface-tagged: the [`web`](Proof::web) block is populated for visual
/// surfaces, the [`bench`](Proof::bench) block for bench surfaces, and a
/// terminal surface carries neither (its snapshot rides on a `web`-less proof,
/// or a `web` block carrying only a `screenshot_url`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Proof {
    /// The surface this proof measures — routes which block is authoritative.
    pub surface: Surface,
    /// The web-vitals + audit block (visual surfaces).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<WebProof>,
    /// The benchmark block (bench surfaces).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bench: Option<BenchProof>,
}

impl Proof {
    /// A proof for a visual surface, carrying its web-vitals + audit block.
    pub fn web(surface: Surface, web: WebProof) -> Self {
        Proof {
            surface,
            web: Some(web),
            bench: None,
        }
    }

    /// A proof for a bench surface, carrying its percentile + throughput block.
    pub fn bench(surface: Surface, bench: BenchProof) -> Self {
        Proof {
            surface,
            web: None,
            bench: Some(bench),
        }
    }

    /// Whether the populated block matches the surface's verification route —
    /// a visual surface should carry `web`, a bench surface `bench`. A terminal
    /// surface is satisfied either way (snapshot-only). Drives a sanity check
    /// before a proof is accepted as evidence.
    pub fn block_matches_surface(&self) -> bool {
        if self.surface.is_visual() {
            self.web.is_some()
        } else if self.surface.is_bench() {
            self.bench.is_some()
        } else {
            true
        }
    }

    /// Whether this proof is real objective EVIDENCE — the matching block is not
    /// only present ([`block_matches_surface`](Proof::block_matches_surface)) but
    /// POPULATED with measurement. This is the stronger bar a Prove/Audit gate
    /// enforces before it may auto-lock: a right-surface-but-empty block (an
    /// agent's `default()` placeholder) matches the surface yet proves nothing,
    /// so it must not clear the gate.
    ///
    /// - visual → a `web` block with at least one vital or audit,
    /// - bench → a `bench` block with at least one populated numeric field,
    /// - terminal → a `web` block carrying the captured snapshot
    ///   (`screenshot_url`) or a measurement.
    pub fn is_evidence(&self) -> bool {
        if self.surface.is_visual() {
            self.web.as_ref().is_some_and(WebProof::has_measurement)
        } else if self.surface.is_bench() {
            self.bench.as_ref().is_some_and(BenchProof::has_measurement)
        } else {
            self.web
                .as_ref()
                .is_some_and(|w| w.screenshot_url.is_some() || w.has_measurement())
        }
    }
}

/// Request body for attaching a run's [`Proof`] — `POST /api/proof/:run`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProofAttachRequest {
    /// The objective evidence to attach.
    pub proof: Proof,
    /// The station the proof was measured at, if scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
}

/// Response body for attaching a run's proof (201 on success).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProofAttachResponse {
    /// Always `true` on success.
    pub ok: bool,
    /// The run the proof was attached to.
    pub run: String,
    /// The surface the proof measures.
    pub surface: Surface,
    /// Whether the proof's block matched its surface's verification route.
    pub block_matches_surface: bool,
}

/// Response body for reading a run's attached proof — `GET /api/proof/:run`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProofGetResponse {
    /// The run the proof belongs to.
    pub run: String,
    /// The station the proof was measured at, if scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
    /// The objective evidence.
    pub proof: Proof,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_tokens_are_snake_case() {
        assert_eq!(serde_json::to_value(Surface::WebUi).unwrap(), "web_ui");
        assert_eq!(serde_json::to_value(Surface::Library).unwrap(), "library");
        let s: Surface = serde_json::from_value(serde_json::json!("mobile")).unwrap();
        assert_eq!(s, Surface::Mobile);
        // Unknown token is rejected.
        let bad: Result<Surface, _> = serde_json::from_value(serde_json::json!("telepathy"));
        assert!(bad.is_err());
    }

    #[test]
    fn surface_route_predicates() {
        assert!(Surface::WebUi.is_visual());
        assert!(Surface::Api.is_bench());
        assert!(Surface::Cli.is_terminal());
        for s in [Surface::Library, Surface::Api, Surface::Data] {
            assert_eq!(Surface::as_str(s), serde_json::to_value(s).unwrap());
        }
    }

    #[test]
    fn web_proof_roundtrips_vitals_and_audits() {
        let mut vitals = BTreeMap::new();
        vitals.insert("lcp".to_string(), 1200.0);
        vitals.insert("cls".to_string(), 0.02);
        let proof = Proof::web(
            Surface::WebUi,
            WebProof {
                vitals,
                audits: vec![
                    AuditResult {
                        name: "contrast".into(),
                        value: "4.8:1".into(),
                        pass: true,
                    },
                    AuditResult {
                        name: "touch-target".into(),
                        value: "40px".into(),
                        pass: false,
                    },
                ],
                screenshot_url: Some("/shot/home.png".into()),
            },
        );
        let json = serde_json::to_value(&proof).unwrap();
        assert_eq!(json["surface"], "web_ui");
        assert_eq!(json["web"]["vitals"]["lcp"], 1200.0);
        assert_eq!(json["web"]["audits"][0]["name"], "contrast");
        assert_eq!(json["web"]["audits"][1]["pass"], false);
        assert!(json.get("bench").is_none(), "web proof omits bench");

        let back: Proof = serde_json::from_value(json).unwrap();
        assert_eq!(back.surface, Surface::WebUi);
        assert!(!back.web.as_ref().unwrap().all_audits_pass());
        assert!(back.block_matches_surface());
    }

    #[test]
    fn bench_proof_roundtrips_percentiles() {
        let proof = Proof::bench(
            Surface::Library,
            BenchProof {
                p50: Some(0.5),
                p95: Some(1.2),
                p99: Some(2.0),
                throughput: Some(50_000.0),
                samples: Some(1000),
            },
        );
        let json = serde_json::to_value(&proof).unwrap();
        assert_eq!(json["surface"], "library");
        assert_eq!(json["bench"]["p95"], 1.2);
        assert_eq!(json["bench"]["throughput"], 50_000.0);
        assert_eq!(json["bench"]["samples"], 1000);
        assert!(json.get("web").is_none(), "bench proof omits web");

        let back: Proof = serde_json::from_value(json).unwrap();
        assert!(back.block_matches_surface());
        assert_eq!(back.bench.unwrap().p99, Some(2.0));
    }

    #[test]
    fn block_matches_surface_catches_mismatch() {
        // A visual surface carrying only a bench block does not match.
        let mismatched = Proof {
            surface: Surface::WebUi,
            web: None,
            bench: Some(BenchProof::default()),
        };
        assert!(!mismatched.block_matches_surface());
        // A terminal surface is satisfied with neither block.
        let terminal = Proof {
            surface: Surface::Cli,
            web: None,
            bench: None,
        };
        assert!(terminal.block_matches_surface());
    }

    #[test]
    fn proof_attach_request_roundtrips() {
        let req = ProofAttachRequest {
            proof: Proof::bench(Surface::Api, BenchProof { p50: Some(3.0), ..Default::default() }),
            station: Some("prove".into()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["proof"]["surface"], "api");
        assert_eq!(json["station"], "prove");
        let back: ProofAttachRequest = serde_json::from_value(json).unwrap();
        assert_eq!(back.proof.surface, Surface::Api);
    }

    #[test]
    fn is_evidence_requires_a_populated_block_not_mere_presence() {
        // A visual surface with an EMPTY web block matches the surface (block is
        // present) but is NOT evidence — nothing was measured.
        let empty_web = Proof::web(Surface::WebUi, WebProof::default());
        assert!(empty_web.block_matches_surface(), "empty block still matches route");
        assert!(!empty_web.is_evidence(), "empty block is not evidence");

        // One captured vital is enough to count.
        let mut vitals = BTreeMap::new();
        vitals.insert("lcp".to_string(), 1200.0);
        let real_web = Proof::web(Surface::WebUi, WebProof { vitals, ..Default::default() });
        assert!(real_web.is_evidence());

        // A visual surface whose only signal is a screenshot is still NOT a
        // measurement — the vitals/audits were never run.
        let shot_only = Proof::web(
            Surface::Desktop,
            WebProof { screenshot_url: Some("/s.png".into()), ..Default::default() },
        );
        assert!(!shot_only.is_evidence(), "a lone screenshot is not a visual measurement");

        // A bench surface with an all-None block matches the route but is empty.
        let empty_bench = Proof::bench(Surface::Api, BenchProof::default());
        assert!(empty_bench.block_matches_surface());
        assert!(!empty_bench.is_evidence(), "all-None bench block is not evidence");
        let real_bench = Proof::bench(Surface::Api, BenchProof { p95: Some(2.0), ..Default::default() });
        assert!(real_bench.is_evidence());

        // A terminal surface is satisfied by its captured snapshot.
        let snapshot = Proof {
            surface: Surface::Cli,
            web: Some(WebProof { screenshot_url: Some("/out.txt".into()), ..Default::default() }),
            bench: None,
        };
        assert!(snapshot.is_evidence(), "a terminal snapshot IS its evidence");
        // …but a terminal proof with no block at all is not.
        let bare_terminal = Proof { surface: Surface::Cli, web: None, bench: None };
        assert!(!bare_terminal.is_evidence());
    }

    #[test]
    fn surface_as_str_covers_every_variant() {
        for (s, t) in [
            (Surface::Library, "library"), (Surface::Api, "api"), (Surface::WebUi, "web_ui"),
            (Surface::Tui, "tui"), (Surface::Cli, "cli"), (Surface::Desktop, "desktop"),
            (Surface::Mobile, "mobile"), (Surface::Data, "data"),
        ] {
            assert_eq!(s.as_str(), t);
        }
    }
}
