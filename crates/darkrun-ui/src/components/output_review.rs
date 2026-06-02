//! [`OutputReview`] — annotate a produced OUTPUT screenshot to give FEEDBACK.
//!
//! Distinct from the pre-build design DIRECTION: here the operator reviews a
//! *produced* artifact. They drop pins (relative coordinate + note) on the
//! captured screenshot of the output and leave free-text comments; submitting
//! produces a piece of feedback the fix-worker loop can act on.
//!
//! State is owned by the caller: `pins` is the current pin set over the
//! screenshot, `comments` the current comment lines. `on_place_pin` fires with
//! the pixel offset of a click over the screenshot (the caller normalizes via
//! [`crate::selection::place_pin`]); `on_comment` adds a comment; `on_submit`
//! records the review. The pin geometry lives in [`crate::selection`]; this
//! component is the thin dark-themed shell over it, mirroring the
//! direction-annotation layer but pointed at an output screenshot.

use dioxus::prelude::*;

use crate::components::primitives::{Badge, Button, ButtonVariant, Card};
use crate::kinds::Tone;
use crate::selection::PinPoint;
use crate::tokens;

/// Annotate an OUTPUT screenshot with pins + comments to produce feedback.
#[component]
pub fn OutputReview(
    /// Optional run slug whose output is under review.
    #[props(default)]
    run_slug: Option<String>,
    /// Optional station label.
    #[props(default)]
    station: Option<String>,
    /// The label of the artifact being reviewed.
    #[props(default)]
    artifact_label: Option<String>,
    /// The screenshot url the operator annotates.
    #[props(default)]
    screenshot_url: Option<String>,
    /// Optional prompt rendered above the screenshot.
    #[props(default)]
    prompt: Option<String>,
    /// Pins placed over the screenshot.
    #[props(default)]
    pins: Vec<PinPoint>,
    /// Comment lines on the output.
    #[props(default)]
    comments: Vec<String>,
    /// Whether the review is already submitted / read-only.
    #[props(default = false)]
    submitted: bool,
    /// Pin-placement handler — called with `(offset_x, offset_y, width, height)`
    /// in pixels relative to the screenshot box.
    #[props(default)]
    on_place_pin: Option<EventHandler<(f64, f64, f64, f64)>>,
    /// Comment-submit handler — called with the new comment text.
    #[props(default)]
    on_comment: Option<EventHandler<String>>,
    /// Submit handler.
    #[props(default)]
    on_submit: Option<EventHandler<MouseEvent>>,
) -> Element {
    let scope_label = match (&run_slug, &station) {
        (Some(r), Some(s)) => format!("{r} · {s}"),
        (Some(r), None) => r.clone(),
        (None, Some(s)) => s.clone(),
        (None, None) => "output".to_string(),
    };
    let has_feedback = !pins.is_empty() || !comments.is_empty();

    rsx! {
        Card {
            div { class: "dr-output-review", "data-pin-count": "{pins.len()}",
                div { style: "display:flex;align-items:center;gap:8px;margin-bottom:4px;",
                    Badge { tone: Tone::Info, "visual review" }
                    span {
                        style: format!(
                            "font-family:{};font-size:13px;color:{};",
                            tokens::FONT_MONO, tokens::var::TEXT_MUTED,
                        ),
                        "{scope_label}"
                    }
                    if let Some(label) = artifact_label.clone() {
                        span { style: "flex:1;" }
                        Badge { tone: Tone::Neutral, "{label}" }
                    }
                }

                if let Some(p) = prompt.clone() {
                    if !p.is_empty() {
                        p {
                            style: format!(
                                "margin:6px 0 0;font-family:{};font-size:15px;font-weight:600;\
                                 color:{};line-height:1.35;",
                                tokens::FONT_SANS, tokens::var::TEXT,
                            ),
                            "{p}"
                        }
                    }
                }

                {screenshot_stage(screenshot_url.as_deref(), &pins, submitted, on_place_pin)}

                if !pins.is_empty() {
                    ul {
                        class: "dr-review-pin-list",
                        style: format!(
                            "margin:10px 0 0;padding-left:18px;font-family:{};font-size:12px;color:{};",
                            tokens::FONT_SANS, tokens::var::TEXT_MUTED,
                        ),
                        for (i, pin) in pins.iter().enumerate() {
                            li { style: "margin:2px 0;",
                                span {
                                    style: format!("color:{};font-weight:600;", tokens::var::ACCENT),
                                    "#{i+1} "
                                }
                                "{pin.note}"
                            }
                        }
                    }
                }

                {comment_box(&comments, submitted, on_comment)}

                {submit_bar(pins.len(), comments.len(), has_feedback, submitted, on_submit)}
            }
        }
    }
}

/// The clickable screenshot stage — clicking emits the pixel offset + box size so
/// the caller normalizes into a `0..1` pin. Falls back to a labelled placeholder
/// when no screenshot resolves.
fn screenshot_stage(
    url: Option<&str>,
    pins: &[PinPoint],
    submitted: bool,
    on_place_pin: Option<EventHandler<(f64, f64, f64, f64)>>,
) -> Element {
    let stage = format!(
        "position:relative;margin-top:12px;width:100%;max-width:640px;\
         border-radius:8px;overflow:hidden;border:1px solid {border};\
         background:{base};cursor:{cursor};",
        border = tokens::var::BORDER_STRONG,
        base = tokens::var::SURFACE_BASE,
        cursor = if submitted { "default" } else { "crosshair" },
    );
    let resolved = url.map(str::trim).filter(|u| !u.is_empty());
    rsx! {
        div {
            class: "dr-review-stage",
            style: "{stage}",
            "data-has-shot": "{resolved.is_some()}",
            onclick: move |evt: MouseEvent| {
                if submitted {
                    return;
                }
                let coords = evt.element_coordinates();
                if let Some(h) = &on_place_pin {
                    h.call((coords.x, coords.y, 0.0, 0.0));
                }
            },
            match resolved {
                Some(u) => rsx! {
                    img {
                        style: "display:block;width:100%;pointer-events:none;",
                        src: "{u}",
                        alt: "output screenshot",
                    }
                },
                None => rsx! {
                    div {
                        class: "dr-review-stage-missing",
                        style: format!(
                            "aspect-ratio:16 / 10;display:flex;align-items:center;\
                             justify-content:center;font-family:{};font-size:12px;color:{};",
                            tokens::FONT_MONO, tokens::var::TEXT_FAINT,
                        ),
                        "no screenshot captured"
                    }
                },
            }
            for (i, pin) in pins.iter().enumerate() {
                {pin_marker(i, pin)}
            }
        }
    }
}

/// A single numbered pin marker positioned over the screenshot.
fn pin_marker(index: usize, pin: &PinPoint) -> Element {
    let dot = format!(
        "position:absolute;left:{left};top:{top};transform:translate(-50%,-50%);\
         width:18px;height:18px;border-radius:999px;background:{accent};\
         color:{on};border:2px solid {base};display:flex;align-items:center;\
         justify-content:center;font-family:{mono};font-size:10px;font-weight:700;\
         box-shadow:0 1px 3px rgba(0,0,0,0.5);",
        left = pin.left_pct(),
        top = pin.top_pct(),
        accent = tokens::var::ACCENT,
        on = tokens::var::ON_ACCENT,
        base = tokens::var::SURFACE_BASE,
        mono = tokens::FONT_MONO,
    );
    rsx! {
        div {
            class: "dr-review-pin",
            style: "{dot}",
            "data-pin-index": "{index}",
            title: "{pin.note}",
            "{index + 1}"
        }
    }
}

/// The comment box: a textarea + add button. Each add emits the entered text; the
/// parent owns the comment list.
fn comment_box(
    comments: &[String],
    submitted: bool,
    on_comment: Option<EventHandler<String>>,
) -> Element {
    let mut draft = use_signal(String::new);
    let ta_style = format!(
        "width:100%;min-height:60px;resize:vertical;padding:8px 10px;border-radius:6px;\
         background:{surface};border:1px solid {border};color:{text};\
         font-family:{sans};font-size:13px;",
        surface = tokens::var::SURFACE_BASE,
        border = tokens::var::BORDER,
        text = tokens::var::TEXT,
        sans = tokens::FONT_SANS,
    );
    rsx! {
        div { style: "display:flex;flex-direction:column;gap:8px;margin-top:12px;",
            if !comments.is_empty() {
                div { style: "display:flex;flex-direction:column;gap:6px;",
                    for c in comments.iter() {
                        div {
                            class: "dr-review-comment",
                            style: format!(
                                "padding:8px 10px;border-radius:6px;background:{};border:1px solid {};\
                                 font-family:{};font-size:13px;color:{};",
                                tokens::var::SURFACE_RAISED, tokens::var::BORDER, tokens::FONT_SANS, tokens::var::TEXT,
                            ),
                            "{c}"
                        }
                    }
                }
            }
            if !submitted {
                textarea {
                    class: "dr-review-comment-input",
                    style: "{ta_style}",
                    placeholder: "Add a comment on this output…",
                    value: "{draft}",
                    oninput: move |e| draft.set(e.value()),
                }
                div {
                    Button {
                        variant: ButtonVariant::Secondary,
                        tone: Tone::Accent,
                        disabled: draft.read().trim().is_empty(),
                        on_click: move |_| {
                            let text = draft.read().trim().to_string();
                            if !text.is_empty() {
                                if let Some(h) = &on_comment {
                                    h.call(text);
                                }
                                draft.set(String::new());
                            }
                        },
                        "Add comment"
                    }
                }
            }
        }
    }
}

/// The submit bar — a status line + the primary submit button.
fn submit_bar(
    pin_count: usize,
    comment_count: usize,
    has_feedback: bool,
    submitted: bool,
    on_submit: Option<EventHandler<MouseEvent>>,
) -> Element {
    let bar = format!(
        "display:flex;align-items:center;gap:12px;margin-top:16px;padding-top:14px;\
         border-top:1px solid {border};",
        border = tokens::var::BORDER,
    );
    let status_style = format!(
        "font-family:{mono};font-size:12px;color:{muted};",
        mono = tokens::FONT_MONO,
        muted = tokens::var::TEXT_MUTED,
    );
    let hint_style = format!(
        "font-family:{mono};font-size:11px;color:{faint};text-transform:lowercase;",
        mono = tokens::FONT_MONO,
        faint = tokens::var::TEXT_FAINT,
    );
    rsx! {
        div { class: "dr-review-submit-bar", style: "{bar}",
            span { style: "{hint_style}", "click the screenshot to drop a pin" }
            span { style: "flex:1;" }
            span { style: "{status_style}", "{pin_count} pins · {comment_count} comments" }
            if submitted {
                Badge { tone: Tone::Ok, filled: true, "submitted" }
            } else {
                Button {
                    variant: ButtonVariant::Primary,
                    tone: Tone::Accent,
                    disabled: !has_feedback,
                    on_click: move |evt| {
                        if let Some(h) = &on_submit {
                            h.call(evt);
                        }
                    },
                    "Submit review"
                }
            }
        }
    }
}
