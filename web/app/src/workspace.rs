//! The persistent-login **workspace** — the signed-in web app, styled like the
//! desktop: a full-height left rail (the shared `darkrun_ui` [`Sidebar`]) listing
//! the user's repositories with their darkrun runs nested under each, and a main
//! pane that opens a run's read-only detail when you click it.
//!
//! Unlike the old flat dashboard (which listed repos with the ephemeral provider
//! OAuth token), the workspace is keyed to the DURABLE Firebase identity: it
//! calls the App-backed `GET /api/workspace` with the
//! persisted Firebase ID token as the bearer, so a returning user lands straight
//! here with no provider re-auth. Clicking a run fetches `GET /api/run` for its
//! full committed state.
//!
//! Read-only: this renders what the committed `.darkrun/` tree reveals (reaching
//! a *live* run is the relay attach path, a separate surface).

use darkrun_ui::components::sidebar::{
    RunDot, Sidebar, SidebarAction, SidebarProject, SidebarRun, SidebarUser,
};
use darkrun_ui::prelude::*;
use darkrun_ui::tokens;
use gloo_net::http::Request;
use serde::Deserialize;

use crate::firebase;

/// One darkrun run discovered in a repo's committed `.darkrun/` tree (mirrors the
/// server's `DiscoveredSession`).
#[derive(Clone, PartialEq, Deserialize)]
pub struct DiscoveredSession {
    /// The run identifier — the `.darkrun/<run_id>/` directory name.
    pub run_id: String,
    /// The owner-qualified repository the run was discovered in.
    pub repo: String,
    /// The provider key (`github` | `gitlab`).
    pub provider: String,
}

/// One repository in the workspace, with its runs embedded (mirrors the server's
/// `WorkspaceRepo`).
#[derive(Clone, PartialEq, Deserialize)]
pub struct WorkspaceRepo {
    /// The short repository name.
    pub name: String,
    /// The owner-qualified path (e.g. `jwaldrip/darkrun`).
    pub full_name: String,
    /// The web URL where the repository lives.
    pub url: String,
    /// The provider key (`github` | `gitlab`).
    pub provider: String,
    /// The darkrun runs committed in this repo's `.darkrun/` tree.
    pub runs: Vec<DiscoveredSession>,
}

/// The `GET /api/workspace` response body.
#[derive(Clone, PartialEq, Deserialize)]
struct WorkspaceResponse {
    /// The user's repositories, each with its runs.
    repos: Vec<WorkspaceRepo>,
}

/// One station row in a committed run's detail (mirrors the server's
/// `CommittedStation`).
#[derive(Clone, PartialEq, Deserialize)]
pub struct CommittedStation {
    /// Station name (e.g. `frame`, `build`).
    pub name: String,
    /// Lifecycle status (display string).
    pub status: String,
}

/// The full committed state of one run (mirrors the server's `CommittedRun`).
#[derive(Clone, PartialEq, Deserialize)]
pub struct CommittedRun {
    /// The run id.
    pub run_id: String,
    /// The owner-qualified repository.
    pub repo: String,
    /// Resolved display title (falls back to the run id).
    pub title: String,
    /// The factory driving the run, if recorded.
    #[serde(default)]
    pub factory: Option<String>,
    /// The station the run currently sits on, if recorded.
    #[serde(default)]
    pub active_station: Option<String>,
    /// Lifecycle status (display string), if recorded.
    #[serde(default)]
    pub status: Option<String>,
    /// Every station the run walks, with its status.
    #[serde(default)]
    pub stations: Vec<CommittedStation>,
}

/// The loading state of the workspace (the repo/run tree).
#[derive(Clone, PartialEq)]
enum Load {
    /// The fetch is in flight.
    Loading,
    /// The workspace loaded (possibly empty).
    Loaded(Vec<WorkspaceRepo>),
    /// The fetch failed.
    Failed(String),
}

/// The main pane's current focus: nothing (the welcome surface), or an open run.
#[derive(Clone, PartialEq)]
enum Open {
    /// Nothing selected — the welcome / empty state.
    None,
    /// A run is opening (its detail fetch in flight).
    Loading { repo: String, run_id: String },
    /// A run's committed detail is shown.
    Run(CommittedRun),
    /// A run's detail failed to load.
    Failed { run_id: String, error: String },
}

/// The signed-in workspace, driven by the persisted Firebase ID token
/// (`id_token`). Fetches `GET /api/workspace`, renders the repos + runs in the
/// shared sidebar, and opens a run's read-only detail in the main pane on click.
#[component]
pub fn Workspace(id_token: String) -> Element {
    let mut load = use_signal(|| Load::Loading);
    let open = use_signal(|| Open::None);

    // Fetch the workspace once the token is available.
    {
        let id_token = id_token.clone();
        use_effect(move || {
            let id_token = id_token.clone();
            load.set(Load::Loading);
            spawn(async move {
                match fetch_workspace(&firebase::web_base(), &id_token).await {
                    Ok(repos) => load.set(Load::Loaded(repos)),
                    Err(e) => load.set(Load::Failed(e)),
                }
            });
        });
    }

    let shell = format!(
        "display:flex;height:100vh;background:{};color:{};font-family:{};",
        tokens::SURFACE_BASE, tokens::TEXT, tokens::FONT_SANS,
    );

    // The sidebar projects (repos → runs), and the id of the open run for the
    // active highlight.
    let active_run = match &*open.read() {
        Open::Run(run) => Some(run.run_id.clone()),
        Open::Loading { run_id, .. } => Some(run_id.clone()),
        _ => None,
    };
    let projects = match load() {
        Load::Loaded(ref repos) => sidebar_projects(repos, active_run.as_deref()),
        _ => Vec::new(),
    };

    // Map a clicked run id back to its repo so the detail fetch knows where to
    // read. Run ids are unique per repo but not necessarily across repos, so the
    // lookup keys on the FIRST repo carrying the id (a run row belongs to exactly
    // one project group in the tree).
    let repos_snapshot = match load() {
        Load::Loaded(ref repos) => repos.clone(),
        _ => Vec::new(),
    };

    rsx! {
        div { style: "{shell}",
            Sidebar {
                actions: workspace_actions(),
                projects,
                user: workspace_user(),
                on_action: move |_id: String| {},
                on_project: move |_id: String| {},
                on_run: {
                    let id_token = id_token.clone();
                    let repos = repos_snapshot.clone();
                    let mut open = open;
                    move |run_id: String| {
                        let Some(repo) = repo_for_run(&repos, &run_id) else { return };
                        open.set(Open::Loading { repo: repo.clone(), run_id: run_id.clone() });
                        let id_token = id_token.clone();
                        let mut open = open;
                        spawn(async move {
                            match fetch_run(&firebase::web_base(), &id_token, &repo, &run_id).await {
                                Ok(run) => open.set(Open::Run(run)),
                                Err(e) => open.set(Open::Failed { run_id, error: e }),
                            }
                        });
                    }
                },
                on_settings: move |_| {},
            }
            main { style: "flex:1;min-width:0;overflow:auto;",
                MainPane { load: load(), open: open() }
            }
        }
    }
}

/// The main pane: the workspace's load state until a run is opened, then that
/// run's read-only detail.
#[component]
fn MainPane(load: Load, open: Open) -> Element {
    // A failed / empty WORKSPACE takes precedence — there's nothing to open.
    match (&load, &open) {
        (Load::Loading, _) => rsx! { Centered { title: "Loading your workspace\u{2026}".to_string(), body: String::new() } },
        (Load::Failed(msg), _) => rsx! {
            Centered {
                title: "Couldn't load your workspace".to_string(),
                body: msg.clone(),
            }
        },
        (Load::Loaded(repos), Open::None) if repos.is_empty() => rsx! {
            Centered {
                title: "No repositories yet".to_string(),
                body: "Install the darkrun GitHub App on a repo, or start a run there \u{2014} \
                       it'll show up here.".to_string(),
            }
        },
        (Load::Loaded(_), Open::None) => rsx! { Welcome {} },
        (Load::Loaded(_), Open::Loading { run_id, .. }) => rsx! {
            Centered { title: format!("Opening {run_id}\u{2026}"), body: String::new() }
        },
        (Load::Loaded(_), Open::Failed { run_id, error }) => rsx! {
            Centered {
                title: format!("Couldn't open {run_id}"),
                body: error.clone(),
            }
        },
        (Load::Loaded(_), Open::Run(run)) => rsx! { RunDetail { run: run.clone() } },
    }
}

/// The welcome / empty main-pane surface shown with a loaded workspace and no run
/// open — a short lead pointing at the sidebar.
#[component]
fn Welcome() -> Element {
    rsx! {
        div { style: "padding:48px 32px;max-width:640px;",
            h1 {
                style: format!(
                    "font-family:{};font-size:22px;color:{};margin:0 0 10px;",
                    tokens::FONT_SANS, tokens::TEXT,
                ),
                "Your workspace"
            }
            p {
                style: format!(
                    "font-family:{};font-size:14px;color:{};margin:0;line-height:1.55;",
                    tokens::FONT_SANS, tokens::TEXT_MUTED,
                ),
                "Pick a run from the sidebar to open its committed state \u{2014} its \
                 stations and where it sits. These are read from the repo's committed \
                 .darkrun/ tree; open a live run from the darkrun app to control it."
            }
        }
    }
}

/// One run's read-only detail: identity + status, the station pipeline dots, and
/// the per-station status list — a summary of what the committed run contains.
#[component]
fn RunDetail(run: CommittedRun) -> Element {
    rsx! {
        div { style: "padding:28px 32px;display:flex;flex-direction:column;gap:22px;max-width:820px;",
            // Identity.
            div {
                div {
                    style: format!(
                        "font-family:{};font-size:12px;color:{};letter-spacing:.06em;\
                         text-transform:uppercase;margin:0 0 6px;",
                        tokens::FONT_MONO, tokens::TEXT_FAINT,
                    ),
                    "{run.repo}"
                }
                h1 {
                    style: format!(
                        "font-family:{};font-size:22px;color:{};margin:0 0 10px;",
                        tokens::FONT_SANS, tokens::TEXT,
                    ),
                    "{run.title}"
                }
                div { style: "display:flex;flex-wrap:wrap;gap:8px;",
                    if let Some(factory) = &run.factory {
                        MetaChip { icon: "fa-solid fa-industry".to_string(), text: factory.clone() }
                    }
                    if let Some(status) = &run.status {
                        MetaChip { icon: "fa-solid fa-circle-half-stroke".to_string(), text: status.clone() }
                    }
                    if let Some(station) = &run.active_station {
                        MetaChip { icon: "fa-solid fa-location-dot".to_string(), text: format!("at {station}") }
                    }
                    MetaChip { icon: "fa-solid fa-code-branch".to_string(), text: "read-only \u{b7} from git".to_string() }
                }
            }

            // The station status list (the committed record). Empty is fine — a
            // freshly-created run may have no stations yet.
            if run.stations.is_empty() {
                p {
                    style: format!(
                        "font-family:{};font-size:13px;color:{};margin:0;",
                        tokens::FONT_SANS, tokens::TEXT_MUTED,
                    ),
                    "No station state committed for this run yet."
                }
            } else {
                div { style: "display:flex;flex-direction:column;gap:12px;",
                    h2 {
                        style: format!(
                            "font-family:{};font-size:13px;letter-spacing:.06em;text-transform:uppercase;\
                             color:{};margin:0;",
                            tokens::FONT_MONO, tokens::TEXT_FAINT,
                        ),
                        "Stations"
                    }
                    div { style: "display:flex;flex-direction:column;gap:6px;",
                        for st in run.stations.iter() {
                            StationRow {
                                name: st.name.clone(),
                                status: st.status.clone(),
                                active: run.active_station.as_deref() == Some(st.name.as_str()),
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One station row in the detail: a status dot, the name, and its status label.
#[component]
fn StationRow(name: String, status: String, active: bool) -> Element {
    let dot = status_dot(&status);
    let card = format!(
        "display:flex;align-items:center;gap:12px;padding:10px 14px;border-radius:8px;\
         border:1px solid {};background:{};",
        if active { tokens::ACCENT_STRONG } else { tokens::BORDER },
        tokens::SURFACE_RAISED,
    );
    rsx! {
        div { style: "{card}",
            span { style: format!("width:8px;height:8px;border-radius:50%;flex:none;background:{dot};") }
            span {
                style: format!(
                    "flex:1;font-family:{};font-size:14px;color:{};",
                    tokens::FONT_SANS, tokens::TEXT,
                ),
                "{name}"
            }
            span {
                style: format!(
                    "font-family:{};font-size:11px;letter-spacing:.04em;text-transform:uppercase;color:{};",
                    tokens::FONT_MONO, tokens::TEXT_MUTED,
                ),
                "{status}"
            }
        }
    }
}

/// A small icon + label metadata chip.
#[component]
fn MetaChip(icon: String, text: String) -> Element {
    rsx! {
        span {
            style: format!(
                "display:inline-flex;align-items:center;gap:6px;padding:4px 10px;\
                 border:1px solid {};border-radius:999px;background:{};\
                 font-family:{};font-size:12px;color:{};",
                tokens::BORDER, tokens::SURFACE_RAISED, tokens::FONT_MONO, tokens::TEXT_MUTED,
            ),
            i { class: "{icon}", style: format!("font-size:11px;color:{};", tokens::TEXT_FAINT) }
            "{text}"
        }
    }
}

/// A centered title + body block for the main pane's loading / empty / error
/// states.
#[component]
fn Centered(title: String, body: String) -> Element {
    rsx! {
        div { style: "height:100%;display:flex;align-items:center;justify-content:center;padding:32px;",
            div { style: "max-width:44ch;text-align:center;",
                h1 {
                    style: format!(
                        "font-family:{};font-size:20px;color:{};margin:0 0 10px;",
                        tokens::FONT_SANS, tokens::TEXT,
                    ),
                    "{title}"
                }
                if !body.is_empty() {
                    p {
                        style: format!(
                            "font-family:{};font-size:14px;color:{};margin:0;line-height:1.5;",
                            tokens::FONT_SANS, tokens::TEXT_MUTED,
                        ),
                        "{body}"
                    }
                }
            }
        }
    }
}

/// The top quick-actions for the sidebar. A single, informational "workspace"
/// header row (no navigation of its own — the app is read-only for now).
fn workspace_actions() -> Vec<SidebarAction> {
    vec![SidebarAction {
        id: "workspace".to_string(),
        icon: "fa-solid fa-layer-group".to_string(),
        label: "Workspace".to_string(),
        shortcut: None,
    }]
}

/// The pinned identity for the sidebar footer. The web app doesn't yet surface
/// the account's display name/avatar, so this is a neutral placeholder that keeps
/// the footer (and its settings affordance) present.
fn workspace_user() -> Option<SidebarUser> {
    Some(SidebarUser {
        name: "Signed in".to_string(),
        avatar: None,
    })
}

/// Project the loaded repos into the sidebar's project→runs tree: each repo is a
/// [`SidebarProject`] with its provider brand icon, and each committed run a
/// [`SidebarRun`] (idle status — these are read from git, not a live engine).
fn sidebar_projects(repos: &[WorkspaceRepo], active_run: Option<&str>) -> Vec<SidebarProject> {
    repos
        .iter()
        .map(|repo| SidebarProject {
            id: repo.full_name.clone(),
            name: repo.full_name.clone(),
            icon: provider_icon(&repo.provider),
            runs: repo
                .runs
                .iter()
                .map(|run| SidebarRun {
                    id: run.run_id.clone(),
                    title: run.run_id.clone(),
                    when: String::new(),
                    // Committed runs are known-but-not-live (read from git).
                    status: RunDot::Idle,
                    active: active_run == Some(run.run_id.as_str()),
                })
                .collect(),
        })
        .collect()
}

/// The Font Awesome brand class for a provider key.
fn provider_icon(provider: &str) -> String {
    match provider {
        "gitlab" => "fa-brands fa-gitlab".to_string(),
        _ => "fa-brands fa-github".to_string(),
    }
}

/// The status dot color for a committed station status — green for completed,
/// accent for active/in-progress, amber for blocked, faint for pending/unknown.
fn status_dot(status: &str) -> &'static str {
    match status {
        "completed" => tokens::STATUS_OK,
        "active" | "in_progress" => tokens::ACCENT,
        "blocked" => tokens::STATUS_WARN,
        _ => tokens::TEXT_FAINT,
    }
}

/// The owner-qualified repo a run id belongs to, from the loaded tree.
fn repo_for_run(repos: &[WorkspaceRepo], run_id: &str) -> Option<String> {
    repos.iter().find_map(|repo| {
        repo.runs
            .iter()
            .any(|r| r.run_id == run_id)
            .then(|| repo.full_name.clone())
    })
}

/// Fetch the workspace (`GET /api/workspace`) with the persisted Firebase ID
/// token as the bearer.
async fn fetch_workspace(web_base: &str, id_token: &str) -> Result<Vec<WorkspaceRepo>, String> {
    let url = format!("{}/api/workspace", web_base.trim_end_matches('/'));
    let resp = Request::get(&url)
        .header("Authorization", &format!("Bearer {id_token}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.ok() {
        return Err(server_error(resp.status()));
    }
    resp.json::<WorkspaceResponse>()
        .await
        .map(|r| r.repos)
        .map_err(|e| e.to_string())
}

/// Fetch one run's committed detail (`GET /api/run?repo=&id=`).
async fn fetch_run(
    web_base: &str,
    id_token: &str,
    repo: &str,
    run_id: &str,
) -> Result<CommittedRun, String> {
    let url = format!(
        "{}/api/run?repo={}&id={}",
        web_base.trim_end_matches('/'),
        urlencode(repo),
        urlencode(run_id),
    );
    let resp = Request::get(&url)
        .header("Authorization", &format!("Bearer {id_token}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.ok() {
        return Err(server_error(resp.status()));
    }
    resp.json::<CommittedRun>().await.map_err(|e| e.to_string())
}

/// A short message for a non-2xx server status.
fn server_error(status: u16) -> String {
    match status {
        401 => "Your session expired. Sign in again.".to_string(),
        403 => "This account has no linked GitHub identity.".to_string(),
        503 => "The workspace isn't configured on the server yet.".to_string(),
        other => format!("server returned {other}"),
    }
}

/// Percent-encode a query-parameter value (repo path / run id may carry `/`).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
