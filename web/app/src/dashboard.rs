//! The standalone dashboard — what app.darkrun.ai shows once you sign in
//! directly (no `darkrun login` nonce in the URL).
//!
//! Unlike the CLI-login bridge (which deposits a token and tells you to close
//! the tab), this is the start of the standalone web experience: a signed-in
//! shell with the user's **repository portfolio** — every repo their linked
//! accounts can reach. The list comes from `darkrun-web`'s `GET /api/repos`,
//! which the browser calls with the provider OAuth access token from sign-in.
//!
//! TODO(session-discovery): a later PR reads each repo's committed `.darkrun/`
//! state to surface the runs inside the portfolio (live via the relay, else
//! read-only from git — see the access model). For now a repo is just a link
//! out to the provider.

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

    // Fetch the portfolio once, on first render.
    use_effect(move || {
        let session = session.clone();
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
                    "The repos your linked accounts can reach. Runs inside them land here next."
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
                                RepoRow { repo: repo.clone() }
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

/// One repository row — a link out to the provider. (Session discovery TODO.)
#[component]
fn RepoRow(repo: Repo) -> Element {
    let row = format!(
        "display:flex;align-items:center;gap:12px;padding:12px 14px;text-decoration:none;\
         border:1px solid {};border-radius:8px;background:{};",
        tokens::BORDER, tokens::SURFACE_RAISED,
    );
    rsx! {
        a { href: "{repo.url}", target: "_blank", rel: "noreferrer", style: "{row}",
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
