//! `/review` — how review works.
//!
//! This is **not** the live review surface, and it deliberately does not connect
//! to a running engine. Review is local by default: the **darkrun desktop app**
//! opens a dark-brand window that streams the live session over `ws://127.0.0.1:PORT`
//! and never takes over your browser. Remote review ships too: sign in with
//! `darkrun login` and you can watch and drive the same run from app.darkrun.ai or
//! your phone. It is opt-in, not the default.
//!
//! The page explains that split and shows a representative review layout (built
//! from the real `darkrun-api` types) so the desktop app's surface is legible
//! before you launch it. The `status_tone` mapping is shared with the rest of
//! the site and kept here.

use darkrun_api::review_current::{
    FeedbackSummary, ReviewCurrentPayload, ReviewCurrentStation, ReviewCurrentUnit,
};
use darkrun_api::session::{OutputArtifact, OutputArtifactType};
use darkrun_ui::prelude::*;

use crate::ui::theme;

use crate::ui::SectionHead;

/// One completion criterion shown under a unit: the check text and whether the
/// engine has recorded it as met.
struct Criterion {
    /// The criterion text.
    text: &'static str,
    /// Whether it is satisfied yet.
    met: bool,
}

/// `/review` — the "how review works" explainer.
#[component]
pub fn Review() -> Element {
    let payload = sample_payload();
    let phase = payload.phase.as_deref().and_then(Phase::from_name);

    rsx! {
        SectionHead {
            kicker: "how review works".to_string(),
            title: "Review runs in the desktop app".to_string(),
            lead: Some(
                "Review is a local surface. The darkrun desktop app opens a dark window, \
                 connects to the engine on your machine, and streams the live session. \
                 It never takes over your browser."
                    .to_string(),
            ),
        }

        DesktopNote {}

        div { style: "margin-top:28px;",
            h2 {
                style: format!(
                    "font-family:{};font-size:18px;color:{};margin:0 0 6px;",
                    tokens::FONT_SANS, theme::TEXT,
                ),
                "What the desktop surface shows"
            }
            p {
                style: format!(
                    "font-family:{};font-size:14px;color:{};margin:0 0 18px;max-width:62ch;",
                    tokens::FONT_SANS, theme::TEXT_MUTED,
                ),
                "A representative review, rendered from the real session types. In the app \
                 this is live: the station pipeline, the units and their criteria, declared \
                 outputs, and an approve / request-changes checkpoint."
            }
        }

        // The assembly line: every station on the run, the current one lit, the
        // active station flagged when it carries open feedback.
        div { style: "margin-top:8px;margin-bottom:20px;overflow-x:auto;",
            StationStrip { stations: station_items(&payload) }
        }

        FactoryCard {
            title: format!("run: {}", payload.run),
            factory: "software".to_string(),
            station: payload.station.clone(),
            phase,
            status: Tone::Info,
            status_label: "in review".to_string(),
        }

        // The classified delivery surface, when set at Shape — it routes how
        // Prove/Audit verify this run.
        if let Some(surface) = payload.surface.as_ref() {
            div { style: "margin-top:12px;display:flex;align-items:center;gap:8px;",
                span {
                    style: format!(
                        "font-family:{};font-size:11px;text-transform:uppercase;letter-spacing:0.06em;color:{};",
                        tokens::FONT_MONO, theme::TEXT_FAINT,
                    ),
                    "surface"
                }
                Badge { tone: Tone::Info, "{surface}" }
            }
        }

        div { style: "margin-top:24px;",
            h2 {
                style: format!("font-family:{};font-size:18px;color:{};margin:0 0 10px;", tokens::FONT_SANS, theme::TEXT),
                "Units"
            }
            div { style: "display:flex;flex-direction:column;gap:8px;",
                for unit in payload.units.iter() {
                    UnitRow {
                        title: unit.title.clone(),
                        unit_type: Some("unit".to_string()),
                        status: status_tone(&unit.status),
                        status_label: unit.status.clone(),
                        pass: 1,
                    }
                }
            }
        }

        // The active unit's completion criteria: the checklist the station must
        // satisfy before its checkpoint can advance.
        if let Some(active) = payload.units.first() {
            {criteria_block(&active.title, &sample_criteria())}
        }

        // The outputs each unit declared, rendered from the real OutputArtifact
        // type with an inline preview of the deliverable.
        {declared_outputs(&sample_outputs())}

        div { style: "margin-top:24px;",
            FeedbackCounts {
                pending: payload.feedback_summary.pending,
                addressed: payload.feedback_summary.addressed,
                closed: payload.feedback_summary.closed,
                rejected: payload.feedback_summary.rejected,
            }
        }

        div { style: "margin-top:24px;",
            CheckpointBar { kind: CheckpointKind::Ask, prompt: "Advance the station, or hold for changes?".to_string() }
        }
    }
}

/// The note that frames where review actually happens: local by default in the
/// desktop app, opt-in remote from the web or your phone. Dark-brand, terse.
#[component]
pub fn DesktopNote() -> Element {
    let wrap = format!(
        "border:1px solid {border};border-left:3px solid {accent};border-radius:8px;\
         padding:14px 16px;background:{overlay};",
        border = theme::BORDER,
        accent = theme::ACCENT,
        overlay = theme::SURFACE_OVERLAY,
    );
    let line = format!(
        "font-family:{sans};font-size:14px;color:{text};margin:0;",
        sans = tokens::FONT_SANS,
        text = theme::TEXT,
    );
    let sub = format!(
        "font-family:{sans};font-size:13px;color:{muted};margin:8px 0 0;",
        sans = tokens::FONT_SANS,
        muted = theme::TEXT_MUTED,
    );
    rsx! {
        div { style: "{wrap}",
            div { style: "display:flex;align-items:center;gap:8px;margin-bottom:8px;",
                Badge { tone: Tone::Accent, filled: true, "desktop app" }
                Badge { tone: Tone::Neutral, "remote review: opt-in" }
            }
            p { style: "{line}",
                "Run "
                code {
                    style: format!(
                        "font-family:{};color:{};", tokens::FONT_MONO, theme::ACCENT,
                    ),
                    "darkrun serve"
                }
                " on your machine, then open the desktop app. It lists your runs and opens \
                 any one into its live review over loopback, staying entirely on your box."
            }
            p { style: "{sub}",
                "Want to watch or drive a run from the web or your phone? Sign in with "
                code {
                    style: format!(
                        "font-family:{};color:{};", tokens::FONT_MONO, theme::ACCENT,
                    ),
                    "darkrun login"
                }
                " and the same review reaches you at app.darkrun.ai. Remote review is shipped \
                 and opt-in: local stays the default, and nothing leaves your machine until you \
                 sign in."
            }
        }
    }
}

/// The feedback summary, rendered as tone-coded badges from the real counts.
///
/// Takes primitive counts rather than the `darkrun-api` struct directly because
/// the wire type does not implement `PartialEq` (a Dioxus prop requirement).
#[component]
fn FeedbackCounts(pending: u32, addressed: u32, closed: u32, rejected: u32) -> Element {
    rsx! {
        div { style: "display:flex;gap:10px;flex-wrap:wrap;align-items:center;",
            span {
                style: format!(
                    "font-family:{};font-size:11px;text-transform:uppercase;letter-spacing:0.06em;color:{};",
                    tokens::FONT_MONO, theme::TEXT_FAINT,
                ),
                "feedback"
            }
            Badge { tone: Tone::Warn, "{pending} pending" }
            Badge { tone: Tone::Info, "{addressed} addressed" }
            Badge { tone: Tone::Ok, "{closed} closed" }
            Badge { tone: Tone::Danger, "{rejected} rejected" }
        }
    }
}

/// A small banner marking a not-yet-live scaffold. Kept for reuse by `/browse`.
#[component]
pub fn ScaffoldNote(text: String) -> Element {
    let style = format!(
        "border:1px dashed {border};border-radius:8px;padding:10px 12px;margin:0 0 20px;\
         font-family:{mono};font-size:12px;color:{muted};background:{raised};",
        border = theme::BORDER_STRONG,
        mono = tokens::FONT_MONO,
        muted = theme::TEXT_MUTED,
        raised = theme::SURFACE_RAISED,
    );
    rsx! {
        div { style: "{style}", "{text}" }
    }
}

/// Map a station's display status onto its assembly-line marker state. The
/// engine reports `locked` for a completed-and-sealed station; everything active
/// maps to the current diamond, and unknowns stay pending so a new status never
/// blanks a marker.
fn station_step(status: &str) -> StationStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "locked" | "done" | "complete" | "completed" | "approved" | "passed" => {
            StationStatus::Done
        }
        "active" | "current" | "in_progress" | "in_review" | "review" => StationStatus::Current,
        _ => StationStatus::Pending,
    }
}

/// Project the run's stations onto the assembly-line strip, flagging the current
/// station when the run carries open feedback.
fn station_items(payload: &ReviewCurrentPayload) -> Vec<StationItem> {
    let has_pending = payload.feedback_summary.pending > 0;
    payload
        .stations
        .iter()
        .map(|s| {
            let name = s.label.clone().unwrap_or_else(|| s.name.clone());
            let step = station_step(&s.status);
            if has_pending && step == StationStatus::Current {
                StationItem::with_feedback(name, step)
            } else {
                StationItem::new(name, step)
            }
        })
        .collect()
}

/// The active unit's completion criteria: the checklist the station must satisfy
/// before its checkpoint can advance. Met criteria take a green check; the rest
/// an open ring.
fn criteria_block(unit_title: &str, criteria: &[Criterion]) -> Element {
    let card = format!(
        "margin-top:20px;border:1px solid {border};border-radius:8px;padding:14px 16px;\
         background:{raised};",
        border = theme::BORDER,
        raised = theme::SURFACE_RAISED,
    );
    let head = format!(
        "display:flex;align-items:baseline;gap:10px;margin:0 0 10px;\
         font-family:{sans};font-size:14px;font-weight:700;color:{text};",
        sans = tokens::FONT_SANS,
        text = theme::TEXT,
    );
    let unit_chip = format!(
        "font-family:{mono};font-size:12px;font-weight:400;color:{muted};",
        mono = tokens::FONT_MONO,
        muted = theme::TEXT_MUTED,
    );
    let row = "display:flex;align-items:flex-start;gap:10px;padding:5px 0;";
    let text_style = format!(
        "font-family:{sans};font-size:13px;color:{text};line-height:1.4;",
        sans = tokens::FONT_SANS,
        text = theme::TEXT,
    );
    rsx! {
        div { style: "{card}",
            div { style: "{head}",
                span { "Completion criteria" }
                span { style: "{unit_chip}", "{unit_title}" }
            }
            for c in criteria.iter() {
                div { style: "{row}",
                    if c.met {
                        span {
                            style: format!("color:{};font-size:14px;line-height:1.4;", theme::ACCENT),
                            "\u{2713}"
                        }
                    } else {
                        span {
                            style: format!("color:{};font-size:14px;line-height:1.4;", theme::TEXT_FAINT),
                            "\u{25cb}"
                        }
                    }
                    span { style: "{text_style}", "{c.text}" }
                }
            }
        }
    }
}

/// The outputs each unit declared, rendered from the real [`OutputArtifact`] type
/// with an inline preview of the deliverable.
fn declared_outputs(outputs: &[OutputArtifact]) -> Element {
    let head = format!(
        "font-family:{sans};font-size:18px;color:{text};margin:0 0 10px;",
        sans = tokens::FONT_SANS,
        text = theme::TEXT,
    );
    rsx! {
        div { style: "margin-top:24px;",
            h2 { style: "{head}", "Declared outputs" }
            div { style: "display:flex;flex-direction:column;gap:12px;",
                for o in outputs.iter() {
                    {output_card(o)}
                }
            }
        }
    }
}

/// One declared-output card: file name, render-type badge, station scope, and an
/// inline preview of the content.
fn output_card(o: &OutputArtifact) -> Element {
    let card = format!(
        "border:1px solid {border};border-radius:8px;overflow:hidden;background:{raised};",
        border = theme::BORDER,
        raised = theme::SURFACE_RAISED,
    );
    let bar = format!(
        "display:flex;align-items:center;gap:10px;padding:10px 12px;\
         border-bottom:1px solid {border};",
        border = theme::BORDER,
    );
    let name_style = format!(
        "font-family:{mono};font-size:13px;color:{text};",
        mono = tokens::FONT_MONO,
        text = theme::TEXT,
    );
    let station = if o.station.is_empty() { "run".to_string() } else { o.station.clone() };
    let pre = format!(
        "margin:0;padding:12px;max-height:180px;overflow:auto;\
         font-family:{mono};font-size:12px;line-height:1.5;color:{muted};\
         background:{base};white-space:pre;",
        mono = tokens::FONT_MONO,
        muted = theme::TEXT_MUTED,
        base = theme::SURFACE_BASE,
    );
    rsx! {
        div { style: "{card}",
            div { style: "{bar}",
                span { style: "{name_style}", "{o.name}" }
                Badge { tone: Tone::Neutral, "{output_type_label(o.artifact_type)}" }
                span { style: "flex:1;" }
                span {
                    style: format!(
                        "font-family:{};font-size:11px;text-transform:uppercase;letter-spacing:0.06em;color:{};",
                        tokens::FONT_MONO, theme::TEXT_FAINT,
                    ),
                    "station: {station}"
                }
            }
            if let Some(content) = o.content.as_deref() {
                pre { style: "{pre}", "{content}" }
            }
        }
    }
}

/// A short human label for an output's render kind.
fn output_type_label(kind: OutputArtifactType) -> &'static str {
    match kind {
        OutputArtifactType::Markdown => "markdown",
        OutputArtifactType::Html => "html",
        OutputArtifactType::Image => "image",
        OutputArtifactType::Video => "video",
        OutputArtifactType::Code => "code",
        OutputArtifactType::File => "file",
    }
}

/// Representative completion criteria for the active unit: two met, one still
/// open, so the checklist shows both states.
fn sample_criteria() -> Vec<Criterion> {
    vec![
        Criterion { text: "Refills tokens at the configured steady-state rate.", met: true },
        Criterion {
            text: "Rejects over-budget requests with 429 and a Retry-After header.",
            met: true,
        },
        Criterion {
            text: "Stays correct under concurrent load (bench p99 under 5ms).",
            met: false,
        },
    ]
}

/// Representative declared outputs, built from the real [`OutputArtifact`] type:
/// a spec the unit wrote at Frame and the module it produced at Build.
fn sample_outputs() -> Vec<OutputArtifact> {
    vec![
        OutputArtifact {
            station: "frame".to_string(),
            name: "limiter.spec.md".to_string(),
            artifact_type: OutputArtifactType::Markdown,
            language: None,
            directory: None,
            content: Some(
                "# Token-bucket limiter\n\n\
                 - Per-route budget, refilled at a steady rate.\n\
                 - Over-budget requests get 429 + Retry-After.\n\
                 - Safe under concurrent access."
                    .to_string(),
            ),
            relative_path: None,
            run_relative_path: Some("frame/limiter.spec.md".to_string()),
        },
        OutputArtifact {
            station: "build".to_string(),
            name: "limiter.rs".to_string(),
            artifact_type: OutputArtifactType::Code,
            language: Some("rust".to_string()),
            directory: Some("src".to_string()),
            content: Some(
                "pub fn try_acquire(&self, key: &str) -> Result<(), RetryAfter> {\n\
                 \x20   let bucket = self.buckets.entry(key.into()).or_default();\n\
                 \x20   bucket.refill(Instant::now());\n\
                 \x20   bucket.take_one()\n\
                 }"
                    .to_string(),
            ),
            relative_path: None,
            run_relative_path: Some("build/src/limiter.rs".to_string()),
        },
    ]
}

/// Map a display status string onto a UI tone.
pub fn status_tone(status: &str) -> Tone {
    match status {
        "approved" | "locked" | "done" | "passed" => Tone::Ok,
        "blocked" | "failed" | "rejected" => Tone::Danger,
        "in_review" | "review" | "active" => Tone::Info,
        "pending" | "queued" => Tone::Warn,
        _ => Tone::Neutral,
    }
}

/// A representative payload built from the real `darkrun-api` types.
fn sample_payload() -> ReviewCurrentPayload {
    ReviewCurrentPayload {
        run: "rate-limit-public-api".to_string(),
        station: Some("build".to_string()),
        station_label: None,
        surface: Some("api".to_string()),
        phase: Some("audit".to_string()),
        units: vec![
            ReviewCurrentUnit {
                slug: "limiter-core".to_string(),
                title: "Token-bucket limiter".to_string(),
                status: "in_review".to_string(),
            },
            ReviewCurrentUnit {
                slug: "limiter-middleware".to_string(),
                title: "Axum middleware layer".to_string(),
                status: "pending".to_string(),
            },
            ReviewCurrentUnit {
                slug: "limiter-config".to_string(),
                title: "Per-route config parsing".to_string(),
                status: "approved".to_string(),
            },
        ],
        feedback_summary: FeedbackSummary {
            pending: 2,
            addressed: 1,
            closed: 4,
            rejected: 0,
        },
        stations: vec![
            ReviewCurrentStation {
                name: "frame".to_string(),
                label: None,
                status: "locked".to_string(),
                phase: None,
                iteration: Some(1),
                visits: Some(1),
            },
            ReviewCurrentStation {
                name: "build".to_string(),
                label: None,
                status: "active".to_string(),
                phase: Some("audit".to_string()),
                iteration: Some(2),
                visits: Some(1),
            },
            ReviewCurrentStation {
                name: "prove".to_string(),
                label: None,
                status: "pending".to_string(),
                phase: None,
                iteration: None,
                visits: None,
            },
            ReviewCurrentStation {
                name: "harden".to_string(),
                label: None,
                status: "pending".to_string(),
                phase: None,
                iteration: None,
                visits: None,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn station_step_reads_the_engine_statuses() {
        assert_eq!(station_step("locked"), StationStatus::Done);
        assert_eq!(station_step("done"), StationStatus::Done);
        assert_eq!(station_step("active"), StationStatus::Current);
        assert_eq!(station_step("in_review"), StationStatus::Current);
        // Unknown never blanks a marker.
        assert_eq!(station_step("mystery"), StationStatus::Pending);
    }

    #[test]
    fn station_items_project_the_whole_line() {
        let payload = sample_payload();
        let items = station_items(&payload);
        // The full station line is consumed, not dropped.
        assert_eq!(items.len(), payload.stations.len());
        assert!(items.len() >= 2, "the pipeline shows more than one station");
        let current: Vec<&StationItem> =
            items.iter().filter(|s| s.status == StationStatus::Current).collect();
        assert_eq!(current.len(), 1, "exactly one current station");
        // The run has pending feedback, so the current station flags its dot.
        assert!(current[0].has_feedback, "current station flags open feedback");
        assert!(items.iter().any(|s| s.status == StationStatus::Done));
        assert!(items.iter().any(|s| s.status == StationStatus::Pending));
    }

    #[test]
    fn station_items_do_not_flag_feedback_without_any_pending() {
        let mut payload = sample_payload();
        payload.feedback_summary.pending = 0;
        let items = station_items(&payload);
        assert!(items.iter().all(|s| !s.has_feedback));
    }

    #[test]
    fn sample_criteria_shows_both_met_and_open() {
        let criteria = sample_criteria();
        assert!(criteria.len() >= 2);
        assert!(criteria.iter().any(|c| c.met), "at least one met");
        assert!(criteria.iter().any(|c| !c.met), "at least one still open");
        assert!(criteria.iter().all(|c| !c.text.is_empty()));
    }

    #[test]
    fn sample_outputs_declare_previewable_deliverables() {
        let outputs = sample_outputs();
        assert!(!outputs.is_empty(), "the page must show at least one declared output");
        for o in &outputs {
            assert!(!o.name.is_empty());
            assert!(!o.station.is_empty());
            assert!(o.content.as_deref().is_some_and(|c| !c.is_empty()), "output {} needs a preview", o.name);
        }
    }

    #[test]
    fn output_type_labels_are_stable() {
        assert_eq!(output_type_label(OutputArtifactType::Markdown), "markdown");
        assert_eq!(output_type_label(OutputArtifactType::Code), "code");
        assert_eq!(output_type_label(OutputArtifactType::File), "file");
    }
}
