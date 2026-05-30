//! `/factories` and `/factories/:slug` — rendered from the embedded factory
//! corpus in `darkrun-content`. This is real content, not a stub: every factory
//! the binary ships is listed and detailed here.

use darkrun_ui::prelude::*;

use crate::route::Route;
use crate::ui::SectionHead;

/// `/factories` — the index of every embedded factory.
#[component]
pub fn Factories() -> Element {
    let slugs = darkrun_content::list_factories();
    rsx! {
        SectionHead {
            kicker: "the corpus".to_string(),
            title: "Factories".to_string(),
            lead: Some(
                "A factory is a methodology: an ordered set of stations that take work from intent \
                 to shipped. These ship inside the darkrun binary."
                    .to_string(),
            ),
        }
        if slugs.is_empty() {
            EmptyState {}
        } else {
            div { class: "dr-grid",
                for slug in slugs {
                    FactoryTile { slug }
                }
            }
        }
    }
}

/// A single factory tile, loaded and validated from the corpus.
#[component]
fn FactoryTile(slug: String) -> Element {
    // Load, falling back to a minimal card if validation fails so one bad
    // factory cannot blank the whole index.
    match darkrun_content::load_validated(&slug) {
        Ok(factory) => {
            let desc = factory.frontmatter.description.clone();
            let category = factory.frontmatter.category.clone();
            let station_count = factory.stations.len();
            rsx! {
                Link {
                    to: Route::FactoryDetail { slug: slug.clone() },
                    style: "text-decoration:none;display:block;",
                    Card {
                        div {
                            style: format!(
                                "display:flex;justify-content:space-between;align-items:center;gap:10px;margin-bottom:8px;font-family:{};",
                                tokens::FONT_SANS,
                            ),
                            span {
                                style: format!(
                                    "font-size:17px;font-weight:700;color:{};text-transform:capitalize;",
                                    tokens::TEXT,
                                ),
                                "{slug}"
                            }
                            Badge { tone: Tone::Neutral, "{station_count} stations" }
                        }
                        if !category.is_empty() {
                            div { style: "margin-bottom:8px;",
                                Badge { tone: Tone::Info, "{category}" }
                            }
                        }
                        p {
                            style: format!(
                                "font-family:{};font-size:14px;color:{};margin:0;",
                                tokens::FONT_SANS, tokens::TEXT_MUTED,
                            ),
                            "{desc}"
                        }
                    }
                }
            }
        }
        Err(err) => {
            let msg = err.to_string();
            rsx! {
                Card {
                    span {
                        style: format!("font-family:{};color:{};", tokens::FONT_MONO, tokens::STATUS_WARN),
                        "{slug}: {msg}"
                    }
                }
            }
        }
    }
}

/// `/factories/:slug` — the detail view: overview, then every station with its
/// roles and checkpoint.
#[component]
pub fn FactoryDetail(slug: String) -> Element {
    match darkrun_content::load_validated(&slug) {
        Ok(factory) => {
            let overview = crate::content::render_markdown(&factory.body);
            let default_model = factory.frontmatter.default_model.clone();
            rsx! {
                div { style: "margin-bottom:8px;",
                    Link { to: Route::Factories {},
                        span {
                            style: format!("font-family:{};font-size:13px;color:{};", tokens::FONT_MONO, tokens::ACCENT),
                            "\u{2190} all factories"
                        }
                    }
                }
                SectionHead {
                    kicker: "factory".to_string(),
                    title: slug.clone(),
                    lead: Some(factory.frontmatter.description.clone()),
                }
                if !default_model.is_empty() {
                    div { style: "margin-bottom:16px;",
                        Badge { tone: Tone::Info, "default model: {default_model}" }
                    }
                }
                article { class: "dr-prose", dangerous_inner_html: "{overview}" }

                div { style: "margin-top:32px;display:flex;flex-direction:column;gap:14px;",
                    for (i, station) in factory.stations.iter().enumerate() {
                        StationDetail {
                            index: i,
                            slug: slug.clone(),
                            name: station.name().to_string(),
                            description: station.frontmatter.description.clone(),
                            checkpoint: checkpoint_label(station.checkpoint()),
                            explorers: station.explorers.iter().map(|r| r.name().to_string()).collect(),
                            workers: station.workers.iter().map(|r| r.name().to_string()).collect(),
                            reviewers: station.reviewers.iter().map(|r| r.name().to_string()).collect(),
                        }
                    }
                }
            }
        }
        Err(err) => {
            let msg = err.to_string();
            rsx! {
                SectionHead {
                    kicker: "not found".to_string(),
                    title: slug.clone(),
                    lead: Some(format!("This factory could not be loaded: {msg}")),
                }
                Link { to: Route::Factories {},
                    Button { variant: ButtonVariant::Secondary, "Back to factories" }
                }
            }
        }
    }
}

/// One station block on a factory detail page: its phase, checkpoint, and the
/// explorer/worker/reviewer roster.
#[component]
fn StationDetail(
    index: usize,
    slug: String,
    name: String,
    description: String,
    checkpoint: String,
    explorers: Vec<String>,
    workers: Vec<String>,
    reviewers: Vec<String>,
) -> Element {
    let phase = phase_for_index(index);
    let accent = phase.map(|p| p.hue().base.to_string());
    rsx! {
        Card { accent: accent.clone(),
            div {
                style: format!(
                    "display:flex;justify-content:space-between;align-items:center;gap:12px;margin-bottom:6px;font-family:{};",
                    tokens::FONT_SANS,
                ),
                span {
                    style: format!(
                        "font-size:17px;font-weight:700;color:{};text-transform:capitalize;",
                        tokens::TEXT,
                    ),
                    "{index + 1}. {name}"
                }
                Badge { tone: Tone::Accent, filled: true, "checkpoint: {checkpoint}" }
            }
            if !description.is_empty() {
                p {
                    style: format!("font-family:{};font-size:14px;color:{};margin:0 0 10px;", tokens::FONT_SANS, tokens::TEXT_MUTED),
                    "{description}"
                }
            }
            RoleRow { label: "explorers".to_string(), roles: explorers }
            RoleRow { label: "workers".to_string(), roles: workers }
            RoleRow { label: "reviewers".to_string(), roles: reviewers }
        }
    }
}

/// A labelled row of role chips. Hidden when empty.
#[component]
fn RoleRow(label: String, roles: Vec<String>) -> Element {
    if roles.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { style: "display:flex;align-items:baseline;gap:8px;margin:4px 0;flex-wrap:wrap;",
            span {
                style: format!(
                    "font-family:{};font-size:11px;text-transform:uppercase;letter-spacing:0.06em;color:{};min-width:78px;",
                    tokens::FONT_MONO, tokens::TEXT_FAINT,
                ),
                "{label}"
            }
            for role in roles {
                Badge { tone: Tone::Neutral, "{role}" }
            }
        }
    }
}

/// Empty state when no factories are embedded (defensive — the corpus ships one).
#[component]
fn EmptyState() -> Element {
    rsx! {
        Card {
            p {
                style: format!("font-family:{};color:{};margin:0;", tokens::FONT_SANS, tokens::TEXT_MUTED),
                "No factories are embedded in this build."
            }
        }
    }
}

/// Map a station's position to the phase hue used for its accent stripe.
fn phase_for_index(index: usize) -> Option<Phase> {
    Phase::ALL.get(index).copied()
}

/// Human label for a content-layer checkpoint kind.
fn checkpoint_label(kind: darkrun_core::domain::CheckpointKind) -> String {
    use darkrun_core::domain::CheckpointKind as C;
    match kind {
        C::Auto => "auto",
        C::Ask => "ask",
        C::External => "external",
        C::Await => "await",
    }
    .to_string()
}
