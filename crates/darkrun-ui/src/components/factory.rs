//! Domain-flavored composites: [`FactoryCard`], [`UnitRow`], and [`CheckpointBar`].
//!
//! These wrap the primitives with darkrun vocabulary so screens can be built
//! quickly. They take plain data (strings, enums) — no `darkrun-core` dependency —
//! and the caller maps domain state into them at the boundary.

use dioxus::prelude::*;

use crate::components::pipeline::{strip_for, StationPipeline};
use crate::components::primitives::{Badge, Button, ButtonVariant, Card};
use crate::kinds::{Phase, Tone};
use crate::tokens;

/// A card summarizing a Run's factory: its title, the active station, and the
/// six-phase pipeline for that station.
#[component]
pub fn FactoryCard(
    /// Run / factory title.
    title: String,
    /// The factory (methodology) name shown as a chip.
    factory: String,
    /// The active station name (e.g. "build").
    #[props(default)]
    station: Option<String>,
    /// The active phase within that station, driving the pipeline strip.
    #[props(default)]
    phase: Option<Phase>,
    /// A status tone for the corner badge.
    #[props(default = Tone::Info)]
    status: Tone,
    /// Label for the status badge (e.g. "active", "blocked").
    #[props(default = "active".to_string())]
    status_label: String,
) -> Element {
    let accent = phase.map(|p| p.hue().base.to_string());
    let header =
        "display:flex;align-items:center;justify-content:space-between;gap:12px;margin-bottom:10px;";
    let title_style = format!(
        "font-family:{sans};font-size:15px;font-weight:700;color:{text};",
        sans = tokens::FONT_SANS,
        text = tokens::TEXT,
    );
    let meta_style = format!(
        "display:flex;align-items:center;gap:8px;margin-bottom:12px;\
         font-family:{mono};font-size:12px;color:{muted};",
        mono = tokens::FONT_MONO,
        muted = tokens::TEXT_MUTED,
    );
    rsx! {
        Card { accent: accent,
            div { style: "{header}",
                span { style: "{title_style}", "{title}" }
                Badge { tone: status, filled: true, "{status_label}" }
            }
            div { style: "{meta_style}",
                Badge { tone: Tone::Neutral, "{factory}" }
                if let Some(st) = station.clone() {
                    span { "station: {st}" }
                }
            }
            StationPipeline { dots: strip_for(phase), labels: true }
        }
    }
}

/// A single row representing a Unit: its title, type, status, and pass counter.
///
/// Optional `on_open` makes the whole row a button target.
#[component]
pub fn UnitRow(
    /// Unit title.
    title: String,
    /// Unit type chip (e.g. "feature", "doc").
    #[props(default)]
    unit_type: Option<String>,
    /// Status tone (e.g. green=completed, cyan=active).
    #[props(default = Tone::Neutral)]
    status: Tone,
    /// Status label text.
    #[props(default = "pending".to_string())]
    status_label: String,
    /// The current Pass index, shown as `pass N` when > 0.
    #[props(default = 0)]
    pass: u32,
    /// Optional open handler — renders a trailing "open" button when set.
    #[props(default)]
    on_open: Option<EventHandler<MouseEvent>>,
) -> Element {
    let row = format!(
        "display:flex;align-items:center;gap:10px;padding:8px 10px;\
         border:1px solid {border};border-radius:6px;background:{surface};",
        border = tokens::BORDER,
        surface = tokens::SURFACE_RAISED,
    );
    let title_style = format!(
        "flex:1;min-width:0;font-family:{sans};font-size:13px;color:{text};\
         overflow:hidden;text-overflow:ellipsis;white-space:nowrap;",
        sans = tokens::FONT_SANS,
        text = tokens::TEXT,
    );
    let pass_style = format!(
        "font-family:{mono};font-size:11px;color:{faint};",
        mono = tokens::FONT_MONO,
        faint = tokens::TEXT_FAINT,
    );
    rsx! {
        div { class: "dr-unit-row", style: "{row}",
            Badge { tone: status, filled: true, "{status_label}" }
            span { style: "{title_style}", title: "{title}", "{title}" }
            if let Some(ty) = unit_type.clone() {
                Badge { tone: Tone::Neutral, "{ty}" }
            }
            if pass > 0 {
                span { style: "{pass_style}", "pass {pass}" }
            }
            if let Some(handler) = on_open {
                Button {
                    variant: ButtonVariant::Ghost,
                    on_click: move |evt| handler.call(evt),
                    "open"
                }
            }
        }
    }
}

/// The checkpoint kind that gates a station. Mirrors
/// `darkrun_core::domain::CheckpointKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CheckpointKind {
    /// Advance automatically.
    #[default]
    Auto,
    /// Ask the operator.
    Ask,
    /// Hand off to an external surface.
    External,
    /// Block until a decision arrives.
    Await,
}

impl CheckpointKind {
    fn name(self) -> &'static str {
        match self {
            CheckpointKind::Auto => "auto",
            CheckpointKind::Ask => "ask",
            CheckpointKind::External => "external",
            CheckpointKind::Await => "await",
        }
    }
}

/// A sticky bar surfaced at a Checkpoint gate. Shows the gate kind in the
/// checkpoint (magenta) hue and offers advance / hold actions for `ask`/`await`
/// gates.
#[component]
pub fn CheckpointBar(
    /// The gate kind.
    kind: CheckpointKind,
    /// Free-text prompt shown to the operator.
    #[props(default = "Checkpoint reached.".to_string())]
    prompt: String,
    /// Advance handler. Rendered for `ask`/`await` gates when set.
    #[props(default)]
    on_advance: Option<EventHandler<MouseEvent>>,
    /// Hold / block handler.
    #[props(default)]
    on_hold: Option<EventHandler<MouseEvent>>,
) -> Element {
    let hue = Phase::Checkpoint.hue();
    let bar = format!(
        "display:flex;align-items:center;gap:12px;padding:10px 14px;\
         background:{surface};border:1px solid {border};\
         border-left:3px solid {accent};border-radius:8px;",
        surface = tokens::SURFACE_OVERLAY,
        border = tokens::BORDER,
        accent = hue.base,
    );
    let prompt_style = format!(
        "flex:1;min-width:0;font-family:{sans};font-size:13px;color:{text};",
        sans = tokens::FONT_SANS,
        text = tokens::TEXT,
    );
    // Auto/external gates are informational; ask/await invite a decision.
    let interactive = matches!(kind, CheckpointKind::Ask | CheckpointKind::Await);
    rsx! {
        div { class: "dr-checkpoint-bar", style: "{bar}", "data-kind": kind.name(),
            Badge { tone: Tone::Neutral, "checkpoint:{kind.name()}" }
            span { style: "{prompt_style}", "{prompt}" }
            if interactive {
                if let Some(hold) = on_hold {
                    Button {
                        variant: ButtonVariant::Secondary,
                        tone: Tone::Warn,
                        on_click: move |evt| hold.call(evt),
                        "hold"
                    }
                }
                if let Some(advance) = on_advance {
                    Button {
                        variant: ButtonVariant::Primary,
                        tone: Tone::Accent,
                        on_click: move |evt| advance.call(evt),
                        "advance"
                    }
                }
            }
        }
    }
}
