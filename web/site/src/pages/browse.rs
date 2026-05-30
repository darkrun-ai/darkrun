//! `/browse` — a scaffold that surfaces the engine's HTTP/WS contract.
//!
//! In the shipped product this is where you browse active and past runs pulled
//! from a running engine. Until that feed is wired, the page renders the real
//! `darkrun_api::ROUTES` table so the contract is visible and the page is wired
//! to the actual API crate, not mocked strings.

use darkrun_api::{HttpMethod, ROUTES};
use darkrun_ui::prelude::*;

use crate::pages::review::ScaffoldNote;
use crate::ui::SectionHead;

/// `/browse` — the run browser scaffold.
#[component]
pub fn Browse() -> Element {
    rsx! {
        SectionHead {
            kicker: "scaffold".to_string(),
            title: "Browse".to_string(),
            lead: Some(
                "Browse active and past runs from a running engine. Until the feed is wired, this \
                 lists the engine's HTTP/WS contract straight from darkrun-api."
                    .to_string(),
            ),
        }
        ScaffoldNote {
            text: "No engine connection yet. Showing darkrun_api::ROUTES — the live contract this \
                   page will call."
                .to_string(),
        }
        div { style: "display:flex;flex-direction:column;gap:6px;",
            for spec in ROUTES.iter() {
                RouteRow {
                    method: format!("{:?}", spec.method),
                    is_ws: spec.method == HttpMethod::Ws,
                    path: spec.path_template.to_string(),
                    summary: spec.summary.to_string(),
                    tag: spec.tag.to_string(),
                }
            }
        }
    }
}

/// One route descriptor row.
#[component]
fn RouteRow(method: String, is_ws: bool, path: String, summary: String, tag: String) -> Element {
    let tone = if is_ws { Tone::Accent } else { Tone::Info };
    let row = format!(
        "display:flex;align-items:center;gap:12px;padding:8px 12px;\
         border:1px solid {border};border-radius:8px;background:{raised};",
        border = tokens::BORDER,
        raised = tokens::SURFACE_RAISED,
    );
    rsx! {
        div { style: "{row}",
            span { style: "min-width:58px;", Badge { tone, filled: true, "{method}" } }
            code {
                style: format!("font-family:{};font-size:13px;color:{};min-width:240px;", tokens::FONT_MONO, tokens::TEXT),
                "{path}"
            }
            span {
                style: format!("font-family:{};font-size:13px;color:{};flex:1;", tokens::FONT_SANS, tokens::TEXT_MUTED),
                "{summary}"
            }
            Badge { tone: Tone::Neutral, "{tag}" }
        }
    }
}
