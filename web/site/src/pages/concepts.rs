//! The concept pages: `/methodology`, `/glossary`, `/lifecycles`. Each renders
//! its embedded markdown; methodology also shows the phase legend.

use darkrun_ui::prelude::*;

use crate::content::{self, CONCEPTS};
use crate::ui::{PhaseLegend, Prose};

/// Render a concept page by slug, or a small fallback if it is missing.
fn concept(slug: &str) -> Element {
    match content::find(CONCEPTS, slug) {
        Some(doc) => rsx! { Prose { doc: *doc } },
        None => rsx! {
            article { class: "dr-prose", h1 { "Not found" } p { "This concept page is unavailable." } }
        },
    }
}

/// `/methodology` — why the line is ordered the way it is, plus the phase legend.
#[component]
pub fn Methodology() -> Element {
    rsx! {
        {concept("methodology")}
        div { style: "margin-top:24px;", PhaseLegend {} }
    }
}

/// `/glossary` — the vocabulary reference.
#[component]
pub fn Glossary() -> Element {
    concept("glossary")
}

/// `/lifecycles` — the path work travels through a factory.
#[component]
pub fn Lifecycles() -> Element {
    concept("lifecycles")
}
