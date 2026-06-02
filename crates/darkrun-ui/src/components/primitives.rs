//! Foundational primitives: [`Card`], [`Badge`], and [`Button`].
//!
//! Each is a thin, inline-styled component that resolves against the dark theme
//! tokens. They take children and an optional click handler where it makes sense
//! so the desktop app and the website can compose them without extra wrappers.

use dioxus::prelude::*;

use crate::kinds::Tone;
use crate::tokens;

/// A layered surface with a hairline border and rounded corners.
///
/// `accent`, when set, paints a left rail in that CSS color — used to tint a
/// card by phase or status.
#[component]
pub fn Card(
    /// Optional left-rail accent color (e.g. a phase hue).
    #[props(default)]
    accent: Option<String>,
    /// Optional explicit padding in pixels (defaults to 16).
    #[props(default = 16)]
    padding: u32,
    children: Element,
) -> Element {
    let rail = match &accent {
        Some(c) => format!("border-left:3px solid {c};"),
        None => format!("border-left:1px solid {};", tokens::var::BORDER),
    };
    let style = format!(
        "background:{surface};border:1px solid {border};{rail}\
         border-radius:8px;padding:{padding}px;color:{text};",
        surface = tokens::var::SURFACE_OVERLAY,
        border = tokens::var::BORDER,
        text = tokens::var::TEXT,
    );
    rsx! {
        div { class: "dr-card", style: "{style}", {children} }
    }
}

/// A small, rounded status/label chip.
///
/// When `filled` is true the badge uses the tone as its background; otherwise it
/// renders as an outline chip with tone-colored text and border.
#[component]
pub fn Badge(
    #[props(default = Tone::Neutral)] tone: Tone,
    #[props(default = false)] filled: bool,
    children: Element,
) -> Element {
    let color = tone.color_var();
    let style = if filled {
        format!(
            "background:{color};color:{on};border:1px solid {color};",
            on = tone.on_var(),
        )
    } else {
        format!("background:transparent;color:{color};border:1px solid {color};")
    };
    let style = format!(
        "{style}display:inline-flex;align-items:center;gap:4px;\
         font-family:{mono};font-size:11px;font-weight:600;line-height:1;\
         padding:3px 7px;border-radius:999px;text-transform:lowercase;\
         letter-spacing:0.02em;white-space:nowrap;",
        mono = tokens::FONT_MONO,
    );
    rsx! {
        span { class: "dr-badge", style: "{style}", {children} }
    }
}

/// A button with three emphasis levels (`primary`, `secondary`, `ghost`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// Filled accent — the primary call to action.
    #[default]
    Primary,
    /// Outlined — a secondary action.
    Secondary,
    /// Borderless — a tertiary / inline action.
    Ghost,
}

/// A themed button. Pass `on_click` to handle presses; `disabled` dims it and
/// blocks the handler.
#[component]
pub fn Button(
    #[props(default = ButtonVariant::Primary)] variant: ButtonVariant,
    #[props(default = Tone::Accent)] tone: Tone,
    #[props(default = false)] disabled: bool,
    #[props(default)] on_click: Option<EventHandler<MouseEvent>>,
    children: Element,
) -> Element {
    let color = tone.color_var();
    let core = match variant {
        ButtonVariant::Primary => {
            format!("background:{color};color:{on};border:1px solid {color};", on = tone.on_var())
        }
        ButtonVariant::Secondary => {
            format!("background:transparent;color:{color};border:1px solid {color};")
        }
        ButtonVariant::Ghost => {
            format!("background:transparent;color:{color};border:1px solid transparent;")
        }
    };
    let opacity = if disabled { "0.45" } else { "1" };
    let cursor = if disabled { "not-allowed" } else { "pointer" };
    let style = format!(
        "{core}font-family:{sans};font-size:13px;font-weight:600;\
         padding:7px 14px;border-radius:6px;line-height:1.2;\
         opacity:{opacity};cursor:{cursor};transition:filter .12s ease;",
        sans = tokens::FONT_SANS,
    );
    rsx! {
        button {
            class: "dr-button",
            style: "{style}",
            disabled,
            onclick: move |evt| {
                if !disabled {
                    if let Some(handler) = &on_click {
                        handler.call(evt);
                    }
                }
            },
            {children}
        }
    }
}
