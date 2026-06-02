//! Render the real components to a static HTML file for visual theme/layout
//! verification. Run: `cargo run -p darkrun-ui --example theme_snapshot`, then
//! open `target/theme_snapshot.html` (force light/dark via the OS or a Chrome
//! `--force-color-profile`/emulated `prefers-color-scheme`).

use darkrun_ui::components::chips::{CheckpointBadge, RiskChip};
use darkrun_ui::components::factory::{CheckpointKind, FactoryCard};
use darkrun_ui::components::phase_machine::PhaseMachine;
use darkrun_ui::components::primitives::{Badge, Card};
use darkrun_ui::components::station_flow::StationFlow;
use darkrun_ui::flow::{Beat, FlowStation};
use darkrun_ui::components::wordmark::{Wordmark, WordmarkVariant};
use darkrun_ui::kinds::{Phase, Tone};
use darkrun_ui::tokens;
use dioxus::prelude::*;

fn page() -> Element {
    rsx! {
        style { "{tokens::THEME_CSS}" }
        div {
            style: "padding:32px;display:flex;flex-direction:column;gap:24px;\
                    background:var(--dr-surface-base);min-height:100vh;",
            // Header logo (interactive lights-out wordmark).
            div { style: "display:flex;align-items:center;gap:16px;",
                Wordmark { variant: WordmarkVariant::OutlinedSolidRun, size: 26.0, interactive: true }
                Wordmark { variant: WordmarkVariant::OutlinedSolidRun, size: 26.0 }
            }
            // A factory card (the Image #8 case).
            FactoryCard {
                title: "Ship the importer".to_string(),
                factory: "software".to_string(),
                station: Some("build".to_string()),
                phase: Some(Phase::Manufacture),
            }
            div { style: "display:flex;gap:8px;",
                Badge { tone: Tone::Info, filled: true, "6 stations" }
                Badge { tone: Tone::Neutral, "engineering" }
                Badge { tone: Tone::Ok, filled: true, "passed" }
            }
            // The station-header chips from Image #11 — checkpoint badge + risk chip.
            div { style: "display:flex;gap:8px;align-items:center;",
                CheckpointBadge { kind: CheckpointKind::External, filled: true }
                CheckpointBadge { kind: CheckpointKind::Auto, filled: true }
                RiskChip { risk: "production risk".to_string() }
            }
            Card {
                div { style: "color:var(--dr-text);", "Plain card body text — must read in both themes." }
            }
            // The station pipeline — checking the inner glyph is centered.
            StationFlow {
                stations: vec![
                    FlowStation::new("frame", CheckpointKind::Ask),
                    FlowStation::new("specify", CheckpointKind::Ask),
                    FlowStation::new("shape", CheckpointKind::Ask),
                    FlowStation::new("build", CheckpointKind::Auto),
                    FlowStation::new("prove", CheckpointKind::Ask),
                    FlowStation::new("harden", CheckpointKind::External),
                ],
                active: Some(2),
            }
            // The phase machine on Review with a SINGLE active beat highlighted
            // (adversarial) — the stepper's per-sub-step behavior.
            PhaseMachine { active: Some(Phase::Review), active_step: Some(Beat::Adversarial), size: 340.0 }
        }
    }
}

fn main() {
    let mut vdom = VirtualDom::new(page);
    vdom.rebuild_in_place();
    let body = dioxus_ssr::render(&vdom);
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" \
         content=\"width=820\"></head><body style=\"margin:0\">{body}</body></html>"
    );
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/theme_snapshot.html");
    std::fs::write(path, html).expect("write snapshot");
    println!("wrote {path}");
}
