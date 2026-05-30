//! `/review` — a scaffold of the live review surface, wired to the real
//! `darkrun-api` wire types.
//!
//! The shipped engine streams a [`darkrun_api::ReviewCurrentPayload`] over the
//! local WebSocket. This page does not yet connect to a running engine; it
//! renders a representative payload built from the real API types so the layout
//! and the type wiring are exercised. Connecting the live feed is the remaining
//! work — see the note rendered on the page.

use darkrun_api::review_current::{
    FeedbackSummary, ReviewCurrentPayload, ReviewCurrentStation, ReviewCurrentUnit,
};
use darkrun_ui::prelude::*;

use crate::ui::SectionHead;

/// `/review` — the review session scaffold.
#[component]
pub fn Review() -> Element {
    let payload = sample_payload();
    let phase = payload
        .phase
        .as_deref()
        .and_then(Phase::from_name);

    rsx! {
        SectionHead {
            kicker: "scaffold".to_string(),
            title: "Review".to_string(),
            lead: Some(
                "The live review surface. Wired to the darkrun-api session types; the WebSocket \
                 feed to a running engine is the remaining work."
                    .to_string(),
            ),
        }

        ScaffoldNote {
            text: "Rendering a representative ReviewCurrentPayload. Point this at \
                   ws://127.0.0.1:PORT/ws/session/:id to go live."
                .to_string(),
        }

        FactoryCard {
            title: format!("run: {}", payload.run),
            factory: "software".to_string(),
            station: payload.station.clone(),
            phase,
            status: Tone::Info,
            status_label: "in review".to_string(),
        }

        div { style: "margin-top:24px;",
            h2 {
                style: format!("font-family:{};font-size:18px;color:{};margin:0 0 10px;", tokens::FONT_SANS, tokens::TEXT),
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
                    tokens::FONT_MONO, tokens::TEXT_FAINT,
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

/// A small banner marking a not-yet-live scaffold.
#[component]
pub fn ScaffoldNote(text: String) -> Element {
    let style = format!(
        "border:1px dashed {border};border-radius:8px;padding:10px 12px;margin:0 0 20px;\
         font-family:{mono};font-size:12px;color:{muted};background:{raised};",
        border = tokens::BORDER_STRONG,
        mono = tokens::FONT_MONO,
        muted = tokens::TEXT_MUTED,
        raised = tokens::SURFACE_RAISED,
    );
    rsx! {
        div { style: "{style}", "{text}" }
    }
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
                status: "locked".to_string(),
                phase: None,
                iteration: Some(1),
                visits: Some(1),
            },
            ReviewCurrentStation {
                name: "build".to_string(),
                status: "active".to_string(),
                phase: Some("audit".to_string()),
                iteration: Some(2),
                visits: Some(1),
            },
        ],
    }
}
