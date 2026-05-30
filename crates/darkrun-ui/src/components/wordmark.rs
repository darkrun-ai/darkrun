//! The darkrun wordmark: **dark** in bold + "run" in regular weight.
//!
//! Two variants:
//! - [`WordmarkVariant::Filled`] — solid accent text, used in the desktop app.
//! - [`WordmarkVariant::Outlined`] — transparent fill with an accent stroke,
//!   used on the website hero.

use dioxus::prelude::*;

use crate::tokens;

/// Which rendering of the wordmark to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WordmarkVariant {
    /// Solid fill (desktop chrome).
    #[default]
    Filled,
    /// Outlined / stroked text (website hero).
    Outlined,
}

/// Render the darkrun wordmark.
///
/// `size` is the font size in CSS pixels (defaults to 24). The "dark" segment is
/// bold and accent-colored; "run" is regular weight in the primary text color.
#[component]
pub fn Wordmark(
    #[props(default = WordmarkVariant::Filled)] variant: WordmarkVariant,
    #[props(default = 24.0)] size: f64,
) -> Element {
    let (dark_style, run_style) = match variant {
        WordmarkVariant::Filled => (
            format!("color:{};font-weight:800;", tokens::ACCENT),
            format!("color:{};font-weight:400;", tokens::TEXT),
        ),
        WordmarkVariant::Outlined => (
            // Transparent fill + accent stroke via text-stroke (webkit) with a
            // color fallback so the glyphs are never invisible if unsupported.
            format!(
                "color:transparent;font-weight:800;\
                 -webkit-text-stroke:1px {accent};text-stroke:1px {accent};",
                accent = tokens::ACCENT
            ),
            format!(
                "color:transparent;font-weight:400;\
                 -webkit-text-stroke:1px {muted};text-stroke:1px {muted};",
                muted = tokens::TEXT_MUTED
            ),
        ),
    };

    let root_style = format!(
        "font-family:{font};font-size:{size}px;letter-spacing:-0.02em;\
         line-height:1;display:inline-flex;align-items:baseline;",
        font = tokens::FONT_SANS,
    );

    rsx! {
        span {
            class: "dr-wordmark",
            "data-variant": match variant {
                WordmarkVariant::Filled => "filled",
                WordmarkVariant::Outlined => "outlined",
            },
            style: "{root_style}",
            "aria-label": "darkrun",
            span { class: "dr-wordmark-dark", style: "{dark_style}", "dark" }
            span { class: "dr-wordmark-run", style: "{run_style}", "run" }
        }
    }
}
