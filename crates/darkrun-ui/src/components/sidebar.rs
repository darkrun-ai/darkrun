//! The shared left-rail sidebar, used by BOTH the desktop app and the web
//! dashboard: top quick-actions, projects with their runs (a status dot + a
//! relative "last activity" time), and a pinned user identity at the bottom.
//!
//! Data-driven: the host passes the actions, the project→runs tree, and the
//! signed-in user; the sidebar renders the chrome and reports clicks. It carries
//! no data-fetching or navigation of its own, so the desktop (engine-backed) and
//! web (repo/git-backed) surfaces can feed it from different sources.

use dioxus::prelude::*;

use crate::tokens;

/// A run's status, shown as a colored dot.
#[derive(Clone, Copy, PartialEq)]
pub enum RunDot {
    /// A live engine is driving it.
    Running,
    /// Parked at a checkpoint waiting on a human.
    Gated,
    /// Sealed / finished.
    Done,
    /// Known but not live (read from git).
    Idle,
}

impl RunDot {
    fn color(self) -> &'static str {
        match self {
            RunDot::Running => tokens::var::ACCENT,
            RunDot::Gated => tokens::var::STATUS_WARN,
            RunDot::Done => tokens::var::STATUS_OK,
            RunDot::Idle => tokens::var::TEXT_FAINT,
        }
    }
}

/// One run row.
#[derive(Clone, PartialEq)]
pub struct SidebarRun {
    /// Stable id passed back to `on_run`.
    pub id: String,
    pub title: String,
    /// Relative time label (e.g. `2m`, `1h`), shown right-aligned.
    pub when: String,
    pub status: RunDot,
    /// The currently-open run — highlighted.
    pub active: bool,
}

/// A project group: a folder header plus its runs.
#[derive(Clone, PartialEq)]
pub struct SidebarProject {
    /// Stable id passed back to `on_project` (e.g. slug or full_name).
    pub id: String,
    pub name: String,
    /// A Font Awesome class for the leading glyph (e.g. `fa-brands fa-github`,
    /// or `fa-solid fa-folder`).
    pub icon: String,
    pub runs: Vec<SidebarRun>,
}

/// A top quick-action (icon + label + optional shortcut hint).
#[derive(Clone, PartialEq)]
pub struct SidebarAction {
    /// Stable id passed back to `on_action`.
    pub id: String,
    /// A Font Awesome class, e.g. `fa-solid fa-plus`.
    pub icon: String,
    pub label: String,
    pub shortcut: Option<String>,
}

/// The signed-in identity pinned at the bottom.
#[derive(Clone, PartialEq)]
pub struct SidebarUser {
    pub name: String,
    /// Avatar image URL; falls back to a monogram when absent.
    pub avatar: Option<String>,
}

/// The shared sidebar.
#[component]
pub fn Sidebar(
    actions: Vec<SidebarAction>,
    projects: Vec<SidebarProject>,
    #[props(default)] user: Option<SidebarUser>,
    on_action: EventHandler<String>,
    on_project: EventHandler<String>,
    on_run: EventHandler<String>,
    #[props(default)] on_settings: EventHandler<()>,
) -> Element {
    let rail = format!(
        "display:flex;flex-direction:column;width:248px;flex:none;height:100%;\
         background:{};border-right:1px solid {};font-family:{};",
        tokens::var::SURFACE_RAISED,
        tokens::var::BORDER,
        tokens::FONT_SANS,
    );

    rsx! {
        aside { style: "{rail}",
            // Top quick-actions.
            div { style: "display:flex;flex-direction:column;padding:12px 8px;gap:2px;",
                for action in actions.iter() {
                    ActionRow { action: action.clone(), on_action }
                }
            }

            // Projects → runs, the scrollable middle.
            div { style: format!("border-top:1px solid {};flex:1;overflow-y:auto;padding:12px 8px;", tokens::var::BORDER),
                div {
                    style: format!(
                        "padding:0 8px 6px;font-size:11px;font-weight:700;letter-spacing:.06em;\
                         text-transform:uppercase;color:{};",
                        tokens::var::TEXT_FAINT,
                    ),
                    "Runs"
                }
                if projects.is_empty() {
                    div {
                        style: format!("padding:8px 10px;font-size:13px;color:{};", tokens::var::TEXT_MUTED),
                        "No projects yet."
                    }
                }
                for project in projects.iter() {
                    ProjectGroup { project: project.clone(), on_project, on_run }
                }
            }

            // Pinned identity.
            if let Some(user) = user {
                UserFooter { user, on_settings }
            }
        }
    }
}

/// One top quick-action row.
#[component]
fn ActionRow(action: SidebarAction, on_action: EventHandler<String>) -> Element {
    let id = action.id.clone();
    rsx! {
        button {
            style: format!(
                "display:flex;align-items:center;gap:10px;width:100%;padding:7px 10px;\
                 border:none;background:transparent;cursor:pointer;border-radius:7px;\
                 font-family:{};font-size:13.5px;color:{};text-align:left;",
                tokens::FONT_SANS, tokens::var::TEXT,
            ),
            onmouseenter: |_| {},
            onclick: move |_| on_action.call(id.clone()),
            i { class: "{action.icon}", style: format!("width:16px;text-align:center;color:{};", tokens::var::TEXT_MUTED) }
            span { style: "flex:1;", "{action.label}" }
            if let Some(sc) = &action.shortcut {
                span { style: format!("font-family:{};font-size:11px;color:{};", tokens::FONT_MONO, tokens::var::TEXT_FAINT), "{sc}" }
            }
        }
    }
}

/// A project header + its runs.
#[component]
fn ProjectGroup(project: SidebarProject, on_project: EventHandler<String>, on_run: EventHandler<String>) -> Element {
    let pid = project.id.clone();
    rsx! {
        div { style: "margin-top:8px;",
            button {
                style: format!(
                    "display:flex;align-items:center;gap:8px;width:100%;padding:5px 10px;\
                     border:none;background:transparent;cursor:pointer;border-radius:6px;\
                     font-family:{};font-size:12.5px;font-weight:600;color:{};text-align:left;",
                    tokens::FONT_SANS, tokens::var::TEXT_MUTED,
                ),
                onclick: move |_| on_project.call(pid.clone()),
                i { class: "{project.icon}", style: format!("width:14px;text-align:center;color:{};", tokens::var::TEXT_FAINT) }
                span {
                    style: "flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;",
                    "{project.name}"
                }
            }
            for run in project.runs.iter() {
                RunRow { run: run.clone(), on_run }
            }
        }
    }
}

/// One run row: status dot, title, relative time.
#[component]
fn RunRow(run: SidebarRun, on_run: EventHandler<String>) -> Element {
    let id = run.id.clone();
    let bg = if run.active { tokens::var::SURFACE_OVERLAY } else { "transparent" };
    let title_color = if run.active { tokens::var::TEXT } else { tokens::var::TEXT_MUTED };
    rsx! {
        button {
            style: format!(
                "display:flex;align-items:center;gap:9px;width:100%;padding:6px 10px 6px 22px;\
                 border:none;background:{};cursor:pointer;border-radius:6px;\
                 font-family:{};font-size:13px;color:{};text-align:left;",
                bg, tokens::FONT_SANS, title_color,
            ),
            onclick: move |_| on_run.call(id.clone()),
            span {
                style: format!("width:7px;height:7px;border-radius:50%;flex:none;background:{};", run.status.color()),
            }
            span {
                style: "flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;",
                "{run.title}"
            }
            span {
                style: format!("font-family:{};font-size:11px;color:{};flex:none;", tokens::FONT_MONO, tokens::var::TEXT_FAINT),
                "{run.when}"
            }
        }
    }
}

/// The pinned user identity at the bottom.
#[component]
fn UserFooter(user: SidebarUser, on_settings: EventHandler<()>) -> Element {
    let monogram = user.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    rsx! {
        div {
            style: format!(
                "display:flex;align-items:center;gap:10px;padding:10px 12px;border-top:1px solid {};",
                tokens::var::BORDER,
            ),
            match &user.avatar {
                Some(url) => rsx! {
                    img {
                        src: "{url}",
                        style: "width:26px;height:26px;border-radius:50%;flex:none;object-fit:cover;",
                    }
                },
                None => rsx! {
                    span {
                        style: format!(
                            "width:26px;height:26px;border-radius:50%;flex:none;display:flex;\
                             align-items:center;justify-content:center;font-size:12px;font-weight:700;\
                             background:{};color:{};",
                            tokens::var::ACCENT, tokens::var::ON_ACCENT,
                        ),
                        "{monogram}"
                    }
                },
            }
            span {
                style: format!("flex:1;font-family:{};font-size:13.5px;color:{};overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", tokens::FONT_SANS, tokens::var::TEXT),
                "{user.name}"
            }
            button {
                style: format!("background:none;border:none;cursor:pointer;color:{};font-size:14px;padding:4px;", tokens::var::TEXT_FAINT),
                "aria-label": "Settings",
                onclick: move |_| on_settings.call(()),
                i { class: "fa-solid fa-gear" }
            }
        }
    }
}
