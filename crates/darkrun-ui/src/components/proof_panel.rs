//! [`ProofPanel`] — the objective-evidence panel that renders a run's [`Proof`]
//! NUMBERS.
//!
//! darkrun's verification is OBJECTIVE MEASUREMENT, not an agent asserting
//! quality. The proof carries surface-routed numbers; this panel makes them
//! visible:
//!
//! - a VISUAL surface ([`ProofMetricKind::Web`]) renders the web vitals as
//!   labelled metrics with pass/fail tints, the audit list, and the captured
//!   screenshot;
//! - a BENCH surface ([`ProofMetricKind::Bench`]) renders the latency
//!   percentiles (p50/p95/p99) + throughput + sample count.
//!
//! The component takes plain, `PartialEq` prop data — the caller maps the
//! `darkrun-api` [`crate::components::proof_panel`] wire `Proof` into a
//! [`ProofView`] at the boundary, exactly as the rest of the design system does.
//! The pure value-formatting + vitals-classification logic lives in
//! [`crate::view`] so it is testable without a renderer.

use dioxus::prelude::*;

use crate::components::primitives::{Badge, Card};
use crate::kinds::Tone;
use crate::tokens;
use crate::view::{vital_label, VitalVerdict};

/// Which measurement block a [`ProofView`] carries — mirrors the surface route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofMetricKind {
    /// A visual surface: web vitals + audits + screenshot.
    Web,
    /// A bench surface: latency percentiles + throughput.
    Bench,
    /// A terminal surface: screenshot/snapshot only.
    Terminal,
}

/// One web-vital metric, classified for display.
#[derive(Debug, Clone, PartialEq)]
pub struct VitalMetric {
    /// The metric key (`lcp`, `fcp`, `cls`, `ttfb`, `inp`).
    pub key: String,
    /// The raw measured value.
    pub value: f64,
    /// The display-formatted value (e.g. `"1.20 s"`, `"0.020"`).
    pub display: String,
    /// The good/needs-improvement/poor verdict against the metric threshold.
    pub verdict: VitalVerdict,
}

/// One objective audit result mirrored for display.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditRow {
    /// The audit name.
    pub name: String,
    /// The measured value string.
    pub value: String,
    /// Whether the audit passed.
    pub pass: bool,
}

/// One bench percentile/throughput stat, pre-formatted.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchStat {
    /// The stat label (`p50`, `p95`, `p99`, `throughput`, `samples`).
    pub label: String,
    /// The display-formatted value (e.g. `"1.20 ms"`, `"50.0k ops/s"`).
    pub display: String,
}

/// The display-ready projection of a wire `Proof` consumed by [`ProofPanel`].
#[derive(Debug, Clone, PartialEq)]
pub struct ProofView {
    /// The surface label (e.g. `"web_ui"`, `"library"`).
    pub surface: String,
    /// Which block is authoritative.
    pub kind: ProofMetricKind,
    /// Web vitals (visual surfaces).
    pub vitals: Vec<VitalMetric>,
    /// Audit results (visual surfaces).
    pub audits: Vec<AuditRow>,
    /// The captured screenshot URL, if any.
    pub screenshot_url: Option<String>,
    /// Bench stats (bench surfaces).
    pub bench: Vec<BenchStat>,
    /// Whether the populated block matched the surface's verification route.
    pub block_matches_surface: bool,
}

/// Render a run's objective evidence — surface-routed numbers — in a single
/// panel so the operator sees the MEASUREMENT, not an assertion.
#[component]
pub fn ProofPanel(
    /// The display-ready proof projection.
    proof: ProofView,
) -> Element {
    let kind_label = match proof.kind {
        ProofMetricKind::Web => "visual",
        ProofMetricKind::Bench => "bench",
        ProofMetricKind::Terminal => "terminal",
    };
    rsx! {
        Card {
            div { class: "dr-proof-panel", "data-surface": "{proof.surface}", "data-kind": "{kind_label}",
                div { style: "display:flex;align-items:center;gap:8px;margin-bottom:6px;",
                    Badge { tone: Tone::Accent, filled: true, "proof" }
                    Badge { tone: Tone::Neutral, "{proof.surface}" }
                    Badge { tone: Tone::Info, "{kind_label}" }
                    if !proof.block_matches_surface {
                        Badge { tone: Tone::Danger, "block mismatch" }
                    }
                }
                {proof_heading("Objective evidence")}

                match proof.kind {
                    ProofMetricKind::Web => web_block(&proof),
                    ProofMetricKind::Bench => bench_block(&proof),
                    ProofMetricKind::Terminal => terminal_block(&proof),
                }
            }
        }
    }
}

/// The web-vitals + audits + screenshot block.
fn web_block(proof: &ProofView) -> Element {
    rsx! {
        if !proof.vitals.is_empty() {
            div { class: "dr-proof-vitals",
                style: "display:grid;grid-template-columns:repeat(auto-fill,minmax(110px,1fr));\
                        gap:10px;margin-top:12px;",
                for v in proof.vitals.iter() {
                    {vital_card(v)}
                }
            }
        }
        if !proof.audits.is_empty() {
            div { style: "margin-top:14px;",
                {proof_heading("Audits")}
                div { class: "dr-proof-audits",
                    style: "display:flex;flex-direction:column;gap:6px;margin-top:8px;",
                    for a in proof.audits.iter() {
                        {audit_row(a)}
                    }
                }
            }
        }
        if let Some(url) = proof.screenshot_url.clone() {
            div { style: "margin-top:14px;",
                {proof_heading("Screenshot")}
                img {
                    class: "dr-proof-screenshot",
                    style: format!(
                        "margin-top:8px;width:100%;max-width:560px;border-radius:8px;\
                         border:1px solid {};display:block;",
                        tokens::var::BORDER,
                    ),
                    src: "{url}",
                    alt: "captured surface",
                    loading: "lazy",
                }
            }
        }
    }
}

/// One web-vital metric card, tinted by its verdict.
fn vital_card(v: &VitalMetric) -> Element {
    let (tone, verdict_label) = match v.verdict {
        VitalVerdict::Good => (tokens::var::STATUS_OK, "good"),
        VitalVerdict::NeedsImprovement => (tokens::var::STATUS_WARN, "fair"),
        VitalVerdict::Poor => (tokens::var::STATUS_DANGER, "poor"),
        VitalVerdict::Unknown => (tokens::var::TEXT_FAINT, "—"),
    };
    let card = format!(
        "display:flex;flex-direction:column;gap:4px;padding:10px;border-radius:8px;\
         background:{surface};border:1px solid {border};border-left:3px solid {tone};",
        surface = tokens::var::SURFACE_RAISED,
        border = tokens::var::BORDER,
    );
    rsx! {
        div { class: "dr-vital", style: "{card}",
            "data-vital": "{v.key}", "data-verdict": "{verdict_label}",
            span {
                style: format!(
                    "font-family:{};font-size:11px;color:{};text-transform:uppercase;letter-spacing:0.05em;",
                    tokens::FONT_MONO, tokens::var::TEXT_FAINT,
                ),
                "{vital_label(&v.key)}"
            }
            span {
                style: format!(
                    "font-family:{};font-size:18px;font-weight:700;color:{};",
                    tokens::FONT_MONO, tokens::var::TEXT,
                ),
                "{v.display}"
            }
            span {
                style: format!(
                    "font-family:{};font-size:10px;font-weight:600;color:{};",
                    tokens::FONT_MONO, tone,
                ),
                "{verdict_label}"
            }
        }
    }
}

/// One audit row — name + measured value + pass/fail chip.
fn audit_row(a: &AuditRow) -> Element {
    let (tone, label) = if a.pass {
        (Tone::Ok, "pass")
    } else {
        (Tone::Danger, "fail")
    };
    let row = format!(
        "display:flex;align-items:center;gap:10px;padding:8px 10px;border-radius:6px;\
         background:{surface};border:1px solid {border};font-family:{mono};font-size:12px;",
        surface = tokens::var::SURFACE_RAISED,
        border = tokens::var::BORDER,
        mono = tokens::FONT_MONO,
    );
    rsx! {
        div { class: "dr-audit", style: "{row}",
            "data-audit": "{a.name}", "data-pass": "{a.pass}",
            span { style: format!("flex:1;color:{};", tokens::var::TEXT), "{a.name}" }
            span { style: format!("color:{};", tokens::var::TEXT_MUTED), "{a.value}" }
            Badge { tone, filled: true, "{label}" }
        }
    }
}

/// The bench percentile + throughput block.
fn bench_block(proof: &ProofView) -> Element {
    if proof.bench.is_empty() {
        return rsx! {
            p {
                style: format!(
                    "margin-top:12px;font-family:{};font-size:12px;color:{};",
                    tokens::FONT_MONO, tokens::var::TEXT_MUTED,
                ),
                "No bench measurements attached."
            }
        };
    }
    rsx! {
        div { class: "dr-proof-bench",
            style: "display:grid;grid-template-columns:repeat(auto-fill,minmax(120px,1fr));\
                    gap:10px;margin-top:12px;",
            for s in proof.bench.iter() {
                div { class: "dr-bench-stat",
                    "data-stat": "{s.label}",
                    style: format!(
                        "display:flex;flex-direction:column;gap:4px;padding:10px;border-radius:8px;\
                         background:{surface};border:1px solid {border};border-left:3px solid {accent};",
                        surface = tokens::var::SURFACE_RAISED,
                        border = tokens::var::BORDER,
                        accent = tokens::var::ACCENT,
                    ),
                    span {
                        style: format!(
                            "font-family:{};font-size:11px;color:{};text-transform:uppercase;letter-spacing:0.05em;",
                            tokens::FONT_MONO, tokens::var::TEXT_FAINT,
                        ),
                        "{s.label}"
                    }
                    span {
                        style: format!(
                            "font-family:{};font-size:18px;font-weight:700;color:{};",
                            tokens::FONT_MONO, tokens::var::TEXT,
                        ),
                        "{s.display}"
                    }
                }
            }
        }
    }
}

/// The terminal-snapshot block — just the captured screenshot, if present.
fn terminal_block(proof: &ProofView) -> Element {
    match proof.screenshot_url.clone() {
        Some(url) => rsx! {
            div { style: "margin-top:12px;",
                {proof_heading("Snapshot")}
                img {
                    class: "dr-proof-screenshot",
                    style: format!(
                        "margin-top:8px;width:100%;max-width:560px;border-radius:8px;\
                         border:1px solid {};display:block;",
                        tokens::var::BORDER,
                    ),
                    src: "{url}",
                    alt: "terminal snapshot",
                    loading: "lazy",
                }
            }
        },
        None => rsx! {
            p {
                style: format!(
                    "margin-top:12px;font-family:{};font-size:12px;color:{};",
                    tokens::FONT_MONO, tokens::var::TEXT_MUTED,
                ),
                "No snapshot attached."
            }
        },
    }
}

/// A small section heading shared by the proof blocks.
fn proof_heading(text: &str) -> Element {
    let style = format!(
        "margin:0;font-family:{sans};font-size:13px;font-weight:700;color:{text};\
         text-transform:uppercase;letter-spacing:0.04em;",
        sans = tokens::FONT_SANS,
        text = tokens::var::TEXT,
    );
    rsx! { h3 { style: "{style}", "{text}" } }
}
