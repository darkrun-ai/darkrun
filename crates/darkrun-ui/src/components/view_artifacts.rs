//! [`ViewArtifacts`] — the output ARTIFACT BROWSER.
//!
//! A run's view session lists its output deliverables; this component renders
//! them as a browsable grid of cards (a kind chip + thumbnail-or-glyph tile +
//! label) with a focused detail pane that picks a per-kind specialized viewer:
//!
//! - `image` / `screenshot` — the picture, full-bleed;
//! - `markdown` — the raw source in a readable mono block;
//! - `json` — the document in a code block;
//! - `file` — a labelled placeholder with the path + a fetch link.
//!
//! Selection is owned by the caller (`focused` is the id of the open artifact);
//! pressing a card calls `on_focus`. A screenshot artifact also surfaces a
//! "Review output" action (`on_review`) that hands the operator off to the
//! [`crate::components::output_review::OutputReview`] annotator.
//!
//! The component takes plain, `PartialEq` prop data — the caller maps the
//! `darkrun-api` `ViewArtifact`s into [`ArtifactEntry`]s at the boundary. The
//! kind classification + glyph logic lives in [`crate::view`].

use dioxus::prelude::*;

use crate::components::primitives::{Badge, Button, ButtonVariant, Card};
use crate::kinds::Tone;
use crate::tokens;
use crate::view::ArtifactKind;

/// One browsable output artifact — the design-system mirror of the wire
/// `ViewArtifact`.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactEntry {
    /// Stable id (echoed on focus / review).
    pub id: String,
    /// Run-relative path on disk.
    pub path: String,
    /// How to render the artifact.
    pub kind: ArtifactKind,
    /// Display label.
    pub label: String,
    /// Optional thumbnail url for the grid tile.
    pub thumbnail_url: Option<String>,
    /// Optional full fetch url.
    pub url: Option<String>,
    /// Optional inline body (markdown source / json text), embedded when small.
    pub body: Option<String>,
}

impl ArtifactEntry {
    /// Construct a minimal entry (id + path + kind + label).
    pub fn new(
        id: impl Into<String>,
        path: impl Into<String>,
        kind: ArtifactKind,
        label: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            path: path.into(),
            kind,
            label: label.into(),
            thumbnail_url: None,
            url: None,
            body: None,
        }
    }

    /// Attach a thumbnail url.
    pub fn with_thumbnail(mut self, url: impl Into<String>) -> Self {
        self.thumbnail_url = Some(url.into());
        self
    }

    /// Attach a fetch url.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Attach an inline body.
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }
}

/// The output ARTIFACT BROWSER: a grid of artifact cards plus a focused detail
/// pane with a per-kind specialized viewer.
#[component]
pub fn ViewArtifacts(
    /// The run slug whose outputs are browsed.
    #[props(default)]
    run_slug: Option<String>,
    /// Optional station narrowing label.
    #[props(default)]
    station: Option<String>,
    /// The browsable artifacts.
    artifacts: Vec<ArtifactEntry>,
    /// The id of the currently-focused artifact, if any.
    #[props(default)]
    focused: Option<String>,
    /// Focus handler — called with the pressed artifact id.
    #[props(default)]
    on_focus: Option<EventHandler<String>>,
    /// Review handler — called with a screenshot artifact id to start a visual
    /// review of the output.
    #[props(default)]
    on_review: Option<EventHandler<String>>,
) -> Element {
    let focused_entry = focused
        .as_ref()
        .and_then(|id| artifacts.iter().find(|a| &a.id == id))
        .cloned();
    let grid = "display:grid;grid-template-columns:repeat(auto-fill,minmax(160px,1fr));\
                gap:12px;margin-top:12px;";
    let scope_label = match (&run_slug, &station) {
        (Some(r), Some(s)) => format!("{r} · {s}"),
        (Some(r), None) => r.clone(),
        (None, Some(s)) => s.clone(),
        (None, None) => "outputs".to_string(),
    };

    rsx! {
        Card {
            div { class: "dr-view-artifacts", "data-artifact-count": "{artifacts.len()}",
                div { style: "display:flex;align-items:center;gap:8px;margin-bottom:4px;",
                    Badge { tone: Tone::Info, "view" }
                    span {
                        style: format!(
                            "font-family:{};font-size:13px;color:{};",
                            tokens::FONT_MONO, tokens::TEXT_MUTED,
                        ),
                        "{scope_label}"
                    }
                    span { style: "flex:1;" }
                    Badge { tone: Tone::Neutral, "{artifacts.len()} artifacts" }
                }

                if artifacts.is_empty() {
                    p {
                        style: format!(
                            "margin-top:10px;font-family:{};font-size:13px;color:{};",
                            tokens::FONT_SANS, tokens::TEXT_MUTED,
                        ),
                        "No output artifacts to browse yet."
                    }
                } else {
                    div { class: "dr-artifact-grid", style: "{grid}",
                        for a in artifacts.iter() {
                            {artifact_tile(a, focused.as_deref() == Some(&a.id), on_focus)}
                        }
                    }
                }

                if let Some(entry) = focused_entry {
                    {artifact_detail(&entry, on_review)}
                }
            }
        }
    }
}

/// One artifact card in the grid — a kind chip, a thumbnail (pictorial) or glyph
/// tile, and the label. The focused card takes an accent ring.
fn artifact_tile(
    a: &ArtifactEntry,
    focused: bool,
    on_focus: Option<EventHandler<String>>,
) -> Element {
    let border = if focused { tokens::ACCENT } else { tokens::BORDER };
    let ring = if focused {
        format!("box-shadow:0 0 0 1px {};", tokens::ACCENT)
    } else {
        String::new()
    };
    let card = format!(
        "display:flex;flex-direction:column;gap:8px;padding:10px;border-radius:8px;\
         background:{surface};border:1px solid {border};{ring}cursor:pointer;\
         text-align:left;width:100%;color:{text};transition:border-color .12s ease;",
        surface = tokens::SURFACE_RAISED,
        border = border,
        text = tokens::TEXT,
    );
    let id = a.id.clone();
    rsx! {
        button {
            class: "dr-artifact-tile",
            style: "{card}",
            "data-artifact-id": "{a.id}",
            "data-kind": "{a.kind.label()}",
            "data-focused": "{focused}",
            "aria-pressed": "{focused}",
            onclick: move |_| {
                if let Some(h) = &on_focus {
                    h.call(id.clone());
                }
            },
            {tile_preview(a)}
            div { style: "display:flex;align-items:center;justify-content:space-between;gap:6px;",
                span {
                    style: format!(
                        "font-family:{};font-size:13px;font-weight:600;color:{};\
                         overflow:hidden;text-overflow:ellipsis;white-space:nowrap;",
                        tokens::FONT_SANS, tokens::TEXT,
                    ),
                    "{a.label}"
                }
                Badge { tone: Tone::Neutral, "{a.kind.label()}" }
            }
        }
    }
}

/// The small preview tile inside a card: a thumbnail for pictorial kinds, a glyph
/// tile otherwise.
fn tile_preview(a: &ArtifactEntry) -> Element {
    let frame = format!(
        "width:100%;aspect-ratio:4 / 3;border-radius:6px;overflow:hidden;\
         background:{base};border:1px solid {border};display:flex;\
         align-items:center;justify-content:center;",
        base = tokens::SURFACE_BASE,
        border = tokens::BORDER,
    );
    match (a.kind.is_pictorial(), a.thumbnail_url.as_deref().or(a.url.as_deref())) {
        (true, Some(url)) if !url.trim().is_empty() => rsx! {
            img {
                style: "{frame}object-fit:cover;",
                src: "{url}",
                alt: "{a.label}",
                loading: "lazy",
            }
        },
        _ => rsx! {
            div {
                class: "dr-artifact-glyph",
                style: format!(
                    "{frame}font-family:{};font-size:28px;color:{};",
                    tokens::FONT_MONO, tokens::TEXT_FAINT,
                ),
                "{a.kind.glyph()}"
            }
        },
    }
}

/// The focused detail pane: a header + a per-kind specialized viewer + (for a
/// screenshot) the "Review output" action.
fn artifact_detail(a: &ArtifactEntry, on_review: Option<EventHandler<String>>) -> Element {
    let wrap = format!(
        "margin-top:16px;padding-top:14px;border-top:1px solid {border};\
         display:flex;flex-direction:column;gap:10px;",
        border = tokens::BORDER,
    );
    let id = a.id.clone();
    let reviewable = a.kind.is_reviewable();
    rsx! {
        div { class: "dr-artifact-detail", style: "{wrap}", "data-detail-id": "{a.id}",
            div { style: "display:flex;align-items:center;gap:8px;",
                Badge { tone: Tone::Accent, filled: true, "{a.kind.label()}" }
                span {
                    style: format!(
                        "flex:1;font-family:{};font-size:13px;font-weight:600;color:{};",
                        tokens::FONT_SANS, tokens::TEXT,
                    ),
                    "{a.label}"
                }
                span {
                    style: format!(
                        "font-family:{};font-size:11px;color:{};",
                        tokens::FONT_MONO, tokens::TEXT_FAINT,
                    ),
                    "{a.path}"
                }
            }

            {kind_viewer(a)}

            if reviewable {
                div {
                    Button {
                        variant: ButtonVariant::Secondary,
                        tone: Tone::Accent,
                        on_click: move |_| {
                            if let Some(h) = &on_review {
                                h.call(id.clone());
                            }
                        },
                        "Review output"
                    }
                }
            }
        }
    }
}

/// The per-kind specialized viewer for the focused artifact.
fn kind_viewer(a: &ArtifactEntry) -> Element {
    match a.kind {
        ArtifactKind::Image | ArtifactKind::Screenshot => picture_viewer(a),
        ArtifactKind::Markdown => text_viewer(a, "markdown"),
        ArtifactKind::Json => text_viewer(a, "json"),
        ArtifactKind::File => file_viewer(a),
    }
}

/// Render a full-bleed picture for an image/screenshot artifact, or a labelled
/// placeholder when no url resolves.
fn picture_viewer(a: &ArtifactEntry) -> Element {
    let frame = format!(
        "width:100%;max-width:640px;border-radius:8px;border:1px solid {};display:block;",
        tokens::BORDER_STRONG,
    );
    match a.url.as_deref().or(a.thumbnail_url.as_deref()) {
        Some(url) if !url.trim().is_empty() => rsx! {
            img {
                class: "dr-artifact-image",
                style: "{frame}",
                src: "{url}",
                alt: "{a.label}",
            }
        },
        _ => rsx! {
            div {
                class: "dr-artifact-image-missing",
                style: format!(
                    "{frame}aspect-ratio:16 / 10;display:flex;align-items:center;\
                     justify-content:center;background:{base};font-family:{mono};\
                     font-size:12px;color:{faint};",
                    base = tokens::SURFACE_BASE,
                    mono = tokens::FONT_MONO,
                    faint = tokens::TEXT_FAINT,
                ),
                "no preview"
            }
        },
    }
}

/// Render a text artifact (markdown source / json) in a mono code block.
fn text_viewer(a: &ArtifactEntry, lang: &str) -> Element {
    let body = a
        .body
        .clone()
        .filter(|b| !b.trim().is_empty())
        .unwrap_or_else(|| "(empty)".to_string());
    let pre = format!(
        "margin:0;max-height:420px;overflow:auto;padding:12px 14px;border-radius:8px;\
         background:{base};border:1px solid {border};color:{text};\
         font-family:{mono};font-size:12px;line-height:1.55;white-space:pre-wrap;\
         word-break:break-word;",
        base = tokens::SURFACE_BASE,
        border = tokens::BORDER,
        text = tokens::TEXT,
        mono = tokens::FONT_MONO,
    );
    rsx! {
        pre {
            class: "dr-artifact-text",
            "data-lang": "{lang}",
            style: "{pre}",
            code { "{body}" }
        }
    }
}

/// Render an opaque-file artifact: a placeholder with the path + a fetch link.
fn file_viewer(a: &ArtifactEntry) -> Element {
    let panel = format!(
        "display:flex;align-items:center;gap:12px;padding:14px;border-radius:8px;\
         background:{base};border:1px solid {border};",
        base = tokens::SURFACE_BASE,
        border = tokens::BORDER,
    );
    rsx! {
        div { class: "dr-artifact-file", style: "{panel}",
            span {
                style: format!("font-family:{};font-size:24px;color:{};", tokens::FONT_MONO, tokens::TEXT_FAINT),
                "{ArtifactKind::File.glyph()}"
            }
            div { style: "flex:1;min-width:0;",
                div {
                    style: format!(
                        "font-family:{};font-size:13px;font-weight:600;color:{};",
                        tokens::FONT_SANS, tokens::TEXT,
                    ),
                    "{a.label}"
                }
                div {
                    style: format!(
                        "font-family:{};font-size:11px;color:{};overflow:hidden;\
                         text-overflow:ellipsis;white-space:nowrap;",
                        tokens::FONT_MONO, tokens::TEXT_FAINT,
                    ),
                    "{a.path}"
                }
            }
            if let Some(url) = a.url.clone() {
                a {
                    class: "dr-artifact-download",
                    href: "{url}",
                    target: "_blank",
                    rel: "noopener",
                    style: format!(
                        "font-family:{};font-size:12px;font-weight:600;color:{};text-decoration:none;",
                        tokens::FONT_SANS, tokens::ACCENT,
                    ),
                    "open ↗"
                }
            }
        }
    }
}
