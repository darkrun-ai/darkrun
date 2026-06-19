//! The standalone dashboard — what app.darkrun.ai shows once you sign in
//! directly (no `darkrun login` nonce in the URL).
//!
//! Unlike the CLI-login bridge (which deposits a token and tells you to close
//! the tab), this is the start of the standalone web experience: a signed-in
//! shell with the user's **repository portfolio** — every repo their linked
//! accounts can reach. The list comes from `darkrun-web`'s `GET /api/repos`,
//! which the browser calls with the provider OAuth access token from sign-in.
//!
//! Each repo also surfaces its **darkrun runs**, discovered read-only from the
//! repo's committed `.darkrun/` tree via `GET /api/repos/sessions` (no live
//! engine, no state sync — see the access model). Reaching a *live* run (the
//! relay attach / write path) is a later PR; for now the rows are read-only.

use darkrun_ui::prelude::*;
use darkrun_ui::tokens;
use gloo_net::http::Request;
use serde::Deserialize;

use crate::firebase::{self, Session};

/// One repository in the portfolio, as `/api/repos` returns it (mirrors the
/// server's `Repo`).
#[derive(Clone, PartialEq, Deserialize)]
pub struct Repo {
    /// The short repository name.
    pub name: String,
    /// The owner-qualified path (e.g. `jwaldrip/darkrun`).
    pub full_name: String,
    /// The web URL where the repository lives.
    pub url: String,
    /// The provider key (`github` | `gitlab`).
    pub provider: String,
}

/// One darkrun run discovered in a repo's committed `.darkrun/` tree, as
/// `/api/repos/sessions` returns it (mirrors the server's `DiscoveredSession`).
#[derive(Clone, PartialEq, Deserialize)]
pub struct DiscoveredSession {
    /// The run identifier — the `.darkrun/<run_id>/` directory name.
    pub run_id: String,
    /// The owner-qualified repository the run was discovered in.
    pub repo: String,
    /// The provider key (`github` | `gitlab`).
    pub provider: String,
}

/// The loading state of the repository list.
#[derive(Clone, PartialEq)]
enum Repos {
    /// The fetch is in flight.
    Loading,
    /// The portfolio loaded (possibly empty).
    Loaded(Vec<Repo>),
    /// The fetch failed.
    Failed(String),
}

/// The signed-in dashboard for `session`. Fetches the portfolio on mount and
/// renders the user's repositories.
#[component]
pub fn Dashboard(session: Session) -> Element {
    let mut repos = use_signal(|| Repos::Loading);

    // Fetch the portfolio once, on first render. Clone for the closure so the
    // prop stays available to hand each `RepoRow` its own copy below.
    let portfolio_session = session.clone();
    use_effect(move || {
        let session = portfolio_session.clone();
        repos.set(Repos::Loading);
        spawn(async move {
            match fetch_repos(&firebase::web_base(), &session).await {
                Ok(list) => repos.set(Repos::Loaded(list)),
                Err(e) => repos.set(Repos::Failed(e)),
            }
        });
    });

    rsx! {
        Shell {
            div { style: "width:100%;max-width:720px;",
                h1 {
                    style: format!(
                        "font-family:{};font-size:20px;color:{};margin:0 0 4px;",
                        tokens::FONT_SANS, tokens::TEXT,
                    ),
                    "Your repositories"
                }
                p {
                    style: format!(
                        "font-family:{};font-size:13px;color:{};margin:0 0 20px;",
                        tokens::FONT_SANS, tokens::TEXT_MUTED,
                    ),
                    "The repos your linked accounts can reach, with the darkrun runs committed in each."
                }
                match repos() {
                    Repos::Loading => rsx! { Note { text: "Loading your repositories\u{2026}".to_string() } },
                    Repos::Failed(msg) => rsx! { Note { text: format!("Couldn't load your repositories: {msg}") } },
                    Repos::Loaded(list) if list.is_empty() => rsx! {
                        Note { text: "No repositories found for this account.".to_string() }
                    },
                    Repos::Loaded(list) => rsx! {
                        div { style: "display:flex;flex-direction:column;gap:8px;",
                            for repo in list.iter() {
                                RepoRow { repo: repo.clone(), session: session.clone() }
                            }
                        }
                    },
                }
            }
        }
    }
}

/// Fetch the signed-in user's repository portfolio from `/api/repos`.
async fn fetch_repos(web_base: &str, session: &Session) -> Result<Vec<Repo>, String> {
    if session.access_token.is_empty() {
        return Err("the provider didn't return an access token to list repos with".to_string());
    }
    let url = format!(
        "{}/api/repos?provider={}",
        web_base.trim_end_matches('/'),
        session.provider,
    );
    let resp = Request::get(&url)
        .header("Authorization", &format!("Bearer {}", session.access_token))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.ok() {
        return Err(format!("server returned {}", resp.status()));
    }
    resp.json::<Vec<Repo>>().await.map_err(|e| e.to_string())
}

/// Discover a repo's darkrun runs from its committed `.darkrun/` tree via
/// `/api/repos/sessions`. Read-only: never reaches a live engine.
async fn fetch_sessions(
    web_base: &str,
    session: &Session,
    full_name: &str,
) -> Result<Vec<DiscoveredSession>, String> {
    if session.access_token.is_empty() {
        return Err("no provider access token to read runs with".to_string());
    }
    let url = format!(
        "{}/api/repos/sessions?provider={}&full_name={}",
        web_base.trim_end_matches('/'),
        session.provider,
        full_name,
    );
    let resp = Request::get(&url)
        .header("Authorization", &format!("Bearer {}", session.access_token))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.ok() {
        return Err(format!("server returned {}", resp.status()));
    }
    resp.json::<Vec<DiscoveredSession>>()
        .await
        .map_err(|e| e.to_string())
}

/// The loading state of a repo's discovered runs.
#[derive(Clone, PartialEq)]
enum Sessions {
    /// The discovery fetch is in flight.
    Loading,
    /// The runs loaded (possibly empty).
    Loaded(Vec<DiscoveredSession>),
    /// The discovery fetch failed (a single repo failing is tolerated — the
    /// row stays, the runs just don't render).
    Failed(String),
}

/// One repository: a link out to the provider, plus the darkrun runs discovered
/// read-only in its committed `.darkrun/` tree. Reaching a *live* run is a
/// later PR — these rows are read-only.
#[component]
fn RepoRow(repo: Repo, session: Session) -> Element {
    let mut sessions = use_signal(|| Sessions::Loading);

    // Discover this repo's runs once, on first render. Clone the inputs the
    // closure captures so `repo`/`session` stay available to the rsx below.
    let discover_session = session.clone();
    let discover_full_name = repo.full_name.clone();
    use_effect(move || {
        let session = discover_session.clone();
        let full_name = discover_full_name.clone();
        sessions.set(Sessions::Loading);
        spawn(async move {
            match fetch_sessions(&firebase::web_base(), &session, &full_name).await {
                Ok(list) => sessions.set(Sessions::Loaded(list)),
                Err(e) => sessions.set(Sessions::Failed(e)),
            }
        });
    });

    let card = format!(
        "display:flex;flex-direction:column;gap:8px;padding:12px 14px;\
         border:1px solid {};border-radius:8px;background:{};",
        tokens::BORDER, tokens::SURFACE_RAISED,
    );
    rsx! {
        div { style: "{card}",
            a {
                href: "{repo.url}",
                target: "_blank",
                rel: "noreferrer",
                style: "display:flex;align-items:center;gap:12px;text-decoration:none;",
                span {
                    style: format!(
                        "flex:1;font-family:{};font-size:14px;color:{};",
                        tokens::FONT_SANS, tokens::TEXT,
                    ),
                    "{repo.full_name}"
                }
                span {
                    style: format!(
                        "font-family:{};font-size:11px;letter-spacing:.06em;text-transform:uppercase;color:{};",
                        tokens::FONT_MONO, tokens::TEXT_FAINT,
                    ),
                    "{repo.provider}"
                }
            }
            match sessions() {
                // While discovering — or on a failed/empty discovery — keep the
                // row quiet: a repo with no runs is the common case.
                Sessions::Loading => rsx! {},
                Sessions::Failed(_) => rsx! {},
                Sessions::Loaded(list) if list.is_empty() => rsx! {},
                Sessions::Loaded(list) => rsx! {
                    div {
                        style: format!(
                            "display:flex;flex-direction:column;gap:4px;border-top:1px solid {};padding-top:8px;",
                            tokens::BORDER,
                        ),
                        for run in list.iter() {
                            SessionRow { run: run.clone() }
                        }
                    }
                },
            }
        }
    }
}

/// One discovered run — a read-only line. (Live attach is a later PR.)
#[component]
fn SessionRow(run: DiscoveredSession) -> Element {
    rsx! {
        div {
            style: "display:flex;align-items:center;gap:10px;padding:4px 0;",
            span {
                style: format!(
                    "flex:1;font-family:{};font-size:12px;color:{};",
                    tokens::FONT_MONO, tokens::TEXT_MUTED,
                ),
                "{run.run_id}"
            }
            span {
                style: format!(
                    "font-family:{};font-size:10px;letter-spacing:.06em;text-transform:uppercase;color:{};",
                    tokens::FONT_MONO, tokens::TEXT_FAINT,
                ),
                "from git"
            }
        }
    }
}

/// A muted single-line note (loading / empty / error).
#[component]
fn Note(text: String) -> Element {
    rsx! {
        p {
            style: format!(
                "font-family:{};font-size:13px;color:{};margin:0;padding:24px 0;",
                tokens::FONT_SANS, tokens::TEXT_MUTED,
            ),
            "{text}"
        }
    }
}

/// The dark page shell (header + content). Matches the login shell so the
/// standalone flow looks like one piece.
#[component]
fn Shell(children: Element) -> Element {
    let shell = format!(
        "min-height:100vh;background:{};color:{};font-family:{};display:flex;flex-direction:column;",
        tokens::SURFACE_BASE, tokens::TEXT, tokens::FONT_SANS,
    );
    rsx! {
        div { style: "{shell}",
            header { style: format!("padding:16px 20px;border-bottom:1px solid {};", tokens::BORDER),
                Wordmark { variant: WordmarkVariant::Outlined }
            }
            main { style: "flex:1;display:flex;justify-content:center;padding:32px 24px;",
                {children}
            }
        }
    }
}
