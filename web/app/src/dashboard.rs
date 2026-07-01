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

use crate::firebase::{self, Account, Session};

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

/// The in-flight state of linking a second provider.
#[derive(Clone, PartialEq)]
enum Linking {
    /// No link in progress.
    Idle,
    /// A `linkWithPopup` round-trip is open.
    Working,
    /// The link failed (popup cancelled, already linked, or the identity belongs
    /// to a different darkrun account) — shown inline so the user can retry.
    Failed(String),
}

/// The signed-in dashboard for `account`. Fetches the COMBINED portfolio (every
/// linked provider's repos) and lets the user link the other provider so one
/// darkrun account spans both GitHub and GitLab. `on_link` hands the newly-linked
/// identity back to the owner of the account signal.
///
/// `account` is a `ReadOnlySignal` so linking a provider (which appends an
/// identity upstream) re-runs the portfolio fetch automatically.
#[component]
pub fn Dashboard(account: ReadSignal<Account>, on_link: EventHandler<Session>) -> Element {
    let mut repos = use_signal(|| Repos::Loading);
    let linking = use_signal(|| Linking::Idle);

    // Re-fetch whenever the set of linked identities changes (reading `account()`
    // makes this effect reactive to the link above).
    use_effect(move || {
        let acc = account();
        repos.set(Repos::Loading);
        spawn(async move {
            match fetch_portfolio(&firebase::web_base(), &acc).await {
                Ok(list) => repos.set(Repos::Loaded(list)),
                Err(e) => repos.set(Repos::Failed(e)),
            }
        });
    });

    // The provider not yet linked, if any — what the "Link …" button offers.
    let missing = account().missing_provider();

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
                        "font-family:{};font-size:13px;color:{};margin:0 0 16px;",
                        tokens::FONT_SANS, tokens::TEXT_MUTED,
                    ),
                    "The repos your linked accounts can reach, with the darkrun runs committed in each."
                }
                LinkedAccounts { account, missing, linking, on_link }
                match repos() {
                    Repos::Loading => rsx! { Note { text: "Loading your repositories\u{2026}".to_string() } },
                    Repos::Failed(msg) => rsx! { Note { text: format!("Couldn't load your repositories: {msg}") } },
                    Repos::Loaded(list) if list.is_empty() => rsx! {
                        Note { text: "No repositories found for this account.".to_string() }
                    },
                    Repos::Loaded(list) => rsx! {
                        div { style: "display:flex;flex-direction:column;gap:8px;",
                            for repo in list.iter() {
                                // Hand each row the identity (token) for ITS provider,
                                // so session discovery uses the matching account.
                                if let Some(identity) = account().identity_for(&repo.provider) {
                                    RepoRow { repo: repo.clone(), session: identity.clone() }
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}

/// The linked-accounts strip: a chip per linked provider, plus a "Link …" button
/// for the other provider so one account can span both GitHub and GitLab.
#[component]
fn LinkedAccounts(
    account: ReadSignal<Account>,
    missing: Option<&'static str>,
    linking: Signal<Linking>,
    on_link: EventHandler<Session>,
) -> Element {
    // `Signal`/`EventHandler` are `Copy`, so this is `Fn` — fine for an onclick.
    let link = move |provider: &'static str| {
        let mut linking = linking;
        linking.set(Linking::Working);
        spawn(async move {
            match firebase::link_provider(provider).await {
                Ok(identity) => {
                    on_link.call(identity);
                    linking.set(Linking::Idle);
                }
                Err(e) => linking.set(Linking::Failed(e)),
            }
        });
    };

    let chip = format!(
        "font-family:{};font-size:11px;letter-spacing:.06em;text-transform:uppercase;\
         color:{};border:1px solid {};border-radius:999px;padding:3px 10px;",
        tokens::FONT_MONO, tokens::TEXT_MUTED, tokens::BORDER,
    );
    let link_btn = format!(
        "font-family:{};font-size:12px;color:{};background:transparent;cursor:pointer;\
         border:1px dashed {};border-radius:999px;padding:3px 10px;",
        tokens::FONT_SANS, tokens::ACCENT, tokens::ACCENT,
    );

    rsx! {
        div { style: "display:flex;flex-wrap:wrap;align-items:center;gap:8px;margin:0 0 20px;",
            for id in account().identities.iter() {
                span { style: "{chip}",
                    i { class: "fa-brands fa-{id.provider}", style: "margin-right:6px;" }
                    "{firebase::provider_label(&id.provider)}"
                }
            }
            match (missing, linking()) {
                (_, Linking::Working) => rsx! {
                    span {
                        style: format!("font-family:{};font-size:12px;color:{};", tokens::FONT_SANS, tokens::TEXT_MUTED),
                        "Linking\u{2026}"
                    }
                },
                (Some(provider), _) => rsx! {
                    button {
                        style: "{link_btn}",
                        onclick: move |_| link(provider),
                        "+ Link {firebase::provider_label(provider)}"
                    }
                },
                (None, _) => rsx! {},
            }
            if let Linking::Failed(msg) = linking() {
                span {
                    style: format!("font-family:{};font-size:12px;color:{};", tokens::FONT_SANS, tokens::TEXT_MUTED),
                    "Couldn't link: {msg}"
                }
            }
        }
    }
}

/// Fetch the COMBINED portfolio across every linked identity. A single provider
/// failing (e.g. an expired token) is tolerated — the others still render; only
/// if EVERY identity fails is the whole fetch an error.
async fn fetch_portfolio(web_base: &str, account: &Account) -> Result<Vec<Repo>, String> {
    let mut all = Vec::new();
    let mut last_err = None;
    let mut any_ok = false;
    for identity in &account.identities {
        match fetch_repos(web_base, identity).await {
            Ok(mut list) => {
                any_ok = true;
                all.append(&mut list);
            }
            Err(e) => {
                last_err = Some(format!("{}: {e}", firebase::provider_label(&identity.provider)));
            }
        }
    }
    if any_ok || account.identities.is_empty() {
        Ok(all)
    } else {
        Err(last_err.unwrap_or_else(|| "no linked accounts".to_string()))
    }
}

/// Fetch one identity's repository list from `/api/repos`.
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
                i {
                    class: "fa-brands fa-{repo.provider}",
                    style: format!("color:{};font-size:16px;", tokens::TEXT_MUTED),
                }
                span {
                    style: format!(
                        "flex:1;font-family:{};font-size:14px;color:{};",
                        tokens::FONT_SANS, tokens::TEXT,
                    ),
                    "{repo.full_name}"
                }
                i {
                    class: "fa-solid fa-arrow-up-right-from-square",
                    style: format!("color:{};font-size:11px;", tokens::TEXT_FAINT),
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
            style: "display:flex;align-items:center;gap:8px;padding:4px 0;",
            i {
                class: "fa-solid fa-diagram-project",
                style: format!("color:{};font-size:11px;", tokens::TEXT_FAINT),
            }
            span {
                style: format!(
                    "flex:1;font-family:{};font-size:12px;color:{};",
                    tokens::FONT_MONO, tokens::TEXT_MUTED,
                ),
                "{run.run_id}"
            }
            span {
                title: "read-only, from the committed .darkrun/ tree",
                style: format!(
                    "font-family:{};font-size:10px;letter-spacing:.06em;text-transform:uppercase;color:{};",
                    tokens::FONT_MONO, tokens::TEXT_FAINT,
                ),
                i { class: "fa-solid fa-code-branch", style: "margin-right:5px;" }
                "git"
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
                Wordmark { variant: WordmarkVariant::OutlinedSolidRun, size: 22.0, interactive: true }
            }
            main { style: "flex:1;display:flex;justify-content:center;padding:32px 24px;",
                {children}
            }
        }
    }
}
