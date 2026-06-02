//! [`StationPipeline`] — the ● ◉ ○ glyph strip that shows a station's progress
//! through the six-phase machine, each glyph painted in its phase hue.

use dioxus::prelude::*;

use crate::kinds::{Phase, Step};
use crate::tokens;

/// One phase's display state within the strip.
#[derive(Debug, Clone, PartialEq)]
pub struct PhaseDot {
    /// The phase this dot represents.
    pub phase: Phase,
    /// Done / active / pending.
    pub step: Step,
}

impl PhaseDot {
    /// Construct a dot.
    pub fn new(phase: Phase, step: Step) -> Self {
        Self { phase, step }
    }
}

/// Build the canonical six-dot strip given the currently-active phase: every
/// phase before `active` is `Done`, `active` is `Active`, the rest `Pending`.
///
/// `active = None` marks the station as not yet started (all pending).
pub fn strip_for(active: Option<Phase>) -> Vec<PhaseDot> {
    let active_idx = active.and_then(|a| Phase::ALL.iter().position(|p| *p == a));
    Phase::ALL
        .into_iter()
        .enumerate()
        .map(|(i, phase)| {
            let step = match active_idx {
                Some(ai) if i < ai => Step::Done,
                Some(ai) if i == ai => Step::Active,
                _ => Step::Pending,
            };
            PhaseDot::new(phase, step)
        })
        .collect()
}

/// Render a phase strip from explicit dots. Use [`strip_for`] for the common case.
///
/// When `labels` is true each glyph is captioned with its phase name in mono.
#[component]
pub fn StationPipeline(
    dots: Vec<PhaseDot>,
    #[props(default = false)] labels: bool,
    #[props(default = 16.0)] size: f64,
) -> Element {
    let root = format!(
        "display:inline-flex;align-items:center;gap:8px;font-family:{mono};",
        mono = tokens::FONT_MONO,
    );
    rsx! {
        div {
            class: "dr-pipeline",
            style: "{root}",
            role: "img",
            "aria-label": "station pipeline",
            for (i, dot) in dots.iter().enumerate() {
                {
                    let hue = dot.phase.hue();
                    let dim = matches!(dot.step, Step::Pending);
                    let glyph_color = if dim { tokens::TEXT_FAINT } else { hue.base };
                    let weight = if matches!(dot.step, Step::Active) { "700" } else { "400" };
                    let glyph_style = format!(
                        "color:{glyph_color};font-size:{size}px;font-weight:{weight};line-height:1;"
                    );
                    let item_style = "display:inline-flex;flex-direction:column;align-items:center;gap:3px;";
                    let label_color = if dim { tokens::TEXT_FAINT } else { tokens::TEXT_MUTED };
                    let label_style = format!("color:{label_color};font-size:10px;line-height:1;");
                    // A muted arrow between phases (phase flows left to right),
                    // mirroring the station strip's connectors. Not before the first.
                    let arrow_style = format!(
                        "color:{};font-size:{}px;line-height:1;",
                        tokens::TEXT_FAINT,
                        size * 0.7,
                    );
                    rsx! {
                        if i > 0 {
                            span { class: "dr-pipeline-arrow", "aria-hidden": "true", style: "{arrow_style}", "\u{2192}" }
                        }
                        span {
                            class: "dr-pipeline-step",
                            "data-phase": dot.phase.name(),
                            "data-step": match dot.step {
                                Step::Done => "done",
                                Step::Active => "active",
                                Step::Pending => "pending",
                            },
                            style: "{item_style}",
                            title: dot.phase.name(),
                            span { style: "{glyph_style}", "{dot.step.glyph()}" }
                            if labels {
                                span { style: "{label_style}", "{dot.phase.name()}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_marks_before_current_and_after() {
        let strip = strip_for(Some(Phase::Manufacture));
        assert_eq!(strip.len(), 6);
        assert_eq!(strip[0].step, Step::Done); // spec
        assert_eq!(strip[1].step, Step::Done); // review
        assert_eq!(strip[2].step, Step::Active); // manufacture
        assert_eq!(strip[3].step, Step::Pending); // audit
        assert_eq!(strip[5].step, Step::Pending); // checkpoint
    }

    #[test]
    fn strip_for_none_is_all_pending() {
        let strip = strip_for(None);
        assert!(strip.iter().all(|d| d.step == Step::Pending));
    }

    #[test]
    fn strip_for_checkpoint_completes_the_line() {
        let strip = strip_for(Some(Phase::Checkpoint));
        assert!(strip[..5].iter().all(|d| d.step == Step::Done));
        assert_eq!(strip[5].step, Step::Active);
    }
}
