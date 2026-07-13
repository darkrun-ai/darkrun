//! Provider sign-in + repo picker for the add-a-project surface.
//!
//! The desktop needs the SAME sign-in surface as the web app: an operator picks a
//! repo to clone, and a private repo requires a provider credential. This module
//! is the native equivalent of `web/app/src/login.rs`'s provider buttons plus the
//! repo list: a dark card of GitHub / GitLab sign-in buttons, and a searchable
//! list of the operator's permitted repositories once signed in.
//!
//! ## Which login flow this drives (there are two, and only one is right here)
//!
//! The repo picker needs the PROVIDER OAuth credential (a GitHub/GitLab access
//! token) persisted in [`CredentialStore`] at `~/.darkrun/credentials`, which is
//! exactly what [`darkrun_vcs::list_repos`] reads (via `CredentialStore::get`) and
//! what `darkrun-git`'s `credentials_for` reads to clone a private repo. That
//! credential is minted by the browser-broker OAuth flow the CLI's `darkrun auth
//! login` drives, i.e. [`darkrun_vcs::login`]: it generates a nonce, opens the
//! browser to `<web>/auth/<provider>/start?state=<nonce>`, polls
//! `<web>/auth/broker/<nonce>`, and `CredentialStore::save`s the resulting
//! [`darkrun_vcs::Credential`]. We reuse that function verbatim.
//!
//! It is emphatically NOT the relay flow (`relay_login`), which mints a
//! remote-ACCESS dial token to `~/.darkrun/relay-token`: the wrong credential for
//! cloning repos.
//!
//! ## Threading
//!
//! [`darkrun_vcs::login`] opens the system browser and BLOCKS polling the broker
//! (up to ~180s), and [`fetch_permitted_repos`] is blocking network I/O, so both
//! run on `tokio::task::spawn_blocking`: the UI thread never blocks, and progress
//! is reflected through signals. The raw token is never stored by us (the store
//! owns it) nor displayed.

// Dioxus components are PascalCase by convention (the `rsx!` macro expects it).
#![allow(non_snake_case)]

use darkrun_ui::prelude::*;
use darkrun_vcs::{CredentialStore, Provider, ReqwestTransport, Repo};

/// The providers with a stored credential right now, read from the default
/// credential store (`~/.darkrun/credentials`). A missing store / read error
/// reads as "none signed in" rather than surfacing an error: the panel then just
/// shows the sign-in buttons. Cheap (a small file read), but callers run it off
/// the UI thread so a slow disk never stalls a frame.
pub fn read_signed_in() -> Vec<Provider> {
    CredentialStore::default_path()
        .ok()
        .and_then(|s| s.list().ok())
        .unwrap_or_default()
}

/// Run the browser-broker PROVIDER sign-in for `provider`, blocking until the
/// browser round-trip completes (or times out). On success the minted
/// [`Credential`](darkrun_vcs::Credential) is saved to `~/.darkrun/credentials` by
/// [`darkrun_vcs::login`] itself, so this returns only `Ok(())` / a readable error,
/// never the token.
///
/// **Blocking**: opens the system browser and polls the broker for up to ~180s, so
/// it must be called on `spawn_blocking`.
fn run_provider_login(provider: Provider) -> Result<(), String> {
    let store = CredentialStore::default_path().map_err(|e| format!("credential store: {e}"))?;
    darkrun_vcs::login(provider, &store).map_err(|e| e.to_string())
}

/// List the operator's permitted repositories across every signed-in provider,
/// normalized and sorted by owner-qualified name.
///
/// **Blocking**: makes provider REST calls through [`ReqwestTransport`], so it runs
/// on `spawn_blocking`. A per-provider failure is collected rather than aborting
/// the whole walk, so one provider being down still lists the other's repos; only
/// when NOTHING could be listed AND at least one provider errored is an error
/// surfaced (a readable message, never a panic or a hang). An empty result with no
/// errors means the signed-in accounts simply have no repos.
pub fn fetch_permitted_repos() -> Result<Vec<Repo>, String> {
    let store = CredentialStore::default_path().map_err(|e| format!("credential store: {e}"))?;
    let providers = store.list().map_err(|e| format!("read credentials: {e}"))?;
    if providers.is_empty() {
        return Ok(Vec::new());
    }
    let transport = ReqwestTransport::new().map_err(|e| format!("http client: {e}"))?;

    let mut all = Vec::new();
    let mut errors = Vec::new();
    for provider in providers {
        // A provider in the list always has a credential, but guard the read
        // anyway (a concurrent logout could race it) rather than unwrap.
        let cred = match store.get(provider) {
            Ok(Some(cred)) => cred,
            Ok(None) => continue,
            Err(e) => {
                errors.push(format!("{}: {e}", provider.display_name()));
                continue;
            }
        };
        match darkrun_vcs::list_repos(&transport, provider, &cred) {
            Ok(mut repos) => all.append(&mut repos),
            Err(e) => errors.push(format!("{}: {e}", provider.display_name())),
        }
    }

    // Owner-qualified, case-insensitive order so the list is scannable and stable
    // across both providers.
    all.sort_by(|a, b| {
        a.full_name
            .to_ascii_lowercase()
            .cmp(&b.full_name.to_ascii_lowercase())
    });

    if all.is_empty() && !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(all)
}

/// The clone URL for a picked repo. [`Repo::url`] is the provider's HTML URL
/// (`https://github.com/o/r`); gix/GitHub accept it, but the canonical git URL
/// carries the `.git` suffix, so append it when absent. A trailing slash is
/// trimmed first so we never produce `…/.git`.
pub fn clone_url_for(repo: &Repo) -> String {
    let base = repo.url.trim_end_matches('/');
    if base.ends_with(".git") {
        base.to_string()
    } else {
        format!("{base}.git")
    }
}

/// The provider's brand mark as an inline SVG (self-contained: the desktop
/// webview does not load Font Awesome, which the website uses for these). `fill`
/// is `currentColor`, so the mark takes the button's text color.
fn provider_icon(provider: Provider) -> Element {
    match provider {
        Provider::GitHub => rsx! {
            svg {
                width: "16",
                height: "16",
                view_box: "0 0 16 16",
                fill: "currentColor",
                "aria-hidden": "true",
                path { d: "M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z" }
            }
        },
        Provider::GitLab => rsx! {
            svg {
                width: "16",
                height: "16",
                view_box: "0 0 24 24",
                fill: "currentColor",
                "aria-hidden": "true",
                path { d: "M23.955 13.587l-1.342-4.135-2.664-8.189a.455.455 0 00-.867 0L16.418 9.45H7.582L4.919 1.263a.455.455 0 00-.867 0L1.386 9.45.044 13.587a.924.924 0 00.331 1.023L12 23.054l11.625-8.443a.92.92 0 00.33-1.024" }
            }
        },
    }
}

/// The sign-in card on the add-a-project pane: per-provider state (a "signed in"
/// chip, or a GitHub / GitLab sign-in button), a busy line while a browser sign-in
/// is in flight, and the last failure if one occurred.
///
/// `signed_in` and `signin_bump` are OWNED by the parent (`AddProjectForm`): the
/// parent gates the repo picker on `signed_in` being non-empty, and the picker
/// refetches whenever `signin_bump` changes. This card's transient status/error is
/// local (the parent doesn't need it).
#[component]
pub fn SignInPanel(signed_in: Signal<Vec<Provider>>, signin_bump: Signal<u32>) -> Element {
    // In-flight text (Some => a browser sign-in is polling; buttons disabled) and
    // the last failure, kept local to the card.
    let status = use_signal(|| None::<String>);
    let error = use_signal(|| None::<String>);

    let providers_in = signed_in.read().clone();
    let busy = status.read().is_some();

    let card = format!(
        "background:{surface};border:1px solid {border};border-radius:8px;\
         padding:14px 16px;display:flex;flex-direction:column;gap:10px;\
         margin-top:12px;",
        surface = tokens::var::SURFACE_OVERLAY,
        border = tokens::var::BORDER,
    );
    let title = format!(
        "font-family:{sans};font-size:13px;font-weight:700;color:{text};",
        sans = tokens::FONT_SANS,
        text = tokens::var::TEXT,
    );
    let body = format!(
        "font-size:12.5px;color:{muted};margin:0;line-height:1.5;",
        muted = tokens::var::TEXT_MUTED,
    );
    let row = "display:flex;flex-wrap:wrap;align-items:center;gap:10px;";
    let signed_chip = format!(
        "display:inline-flex;align-items:center;gap:6px;padding:6px 12px;\
         border:1px solid {border};border-radius:8px;font-family:{sans};\
         font-size:12.5px;color:{text};background:{raised};",
        border = tokens::var::BORDER_STRONG,
        sans = tokens::FONT_SANS,
        text = tokens::var::TEXT,
        raised = tokens::var::SURFACE_RAISED,
    );
    let ok_dot = format!(
        "width:7px;height:7px;border-radius:50%;flex:none;background:{ok};",
        ok = tokens::var::STATUS_OK,
    );

    rsx! {
        div { style: "{card}",
            span { style: "{title}", "Sign in to pick a repo" }
            p { style: "{body}",
                "Sign in with GitHub or GitLab to clone from your permitted repositories. \
                 A private repo needs the credential; darkrun stores it locally in \
                 ~/.darkrun and never shows the token."
            }
            div { style: "{row}",
                for provider in [Provider::GitHub, Provider::GitLab] {
                    if providers_in.contains(&provider) {
                        span { style: "{signed_chip}",
                            span { style: "{ok_dot}" }
                            {provider_icon(provider)}
                            "{provider.display_name()} \u{b7} signed in"
                        }
                    } else {
                        ProviderSignInButton {
                            provider,
                            busy,
                            signed_in,
                            signin_bump,
                            status,
                            error,
                        }
                    }
                }
            }
            if let Some(msg) = status.read().clone() {
                p {
                    style: format!(
                        "font-family:{mono};font-size:11.5px;color:{accent};margin:0;",
                        mono = tokens::FONT_MONO,
                        accent = tokens::var::ACCENT,
                    ),
                    "{msg}"
                }
            }
            if let Some(msg) = error.read().clone() {
                p {
                    style: format!(
                        "font-family:{mono};font-size:11.5px;color:{danger};margin:0;",
                        mono = tokens::FONT_MONO,
                        danger = tokens::var::STATUS_DANGER,
                    ),
                    "{msg}"
                }
            }
        }
    }
}

/// One provider's accent sign-in button. Clicking it runs the browser-broker
/// sign-in on a background thread and reflects progress through `status` /
/// `error`; on success it refreshes `signed_in` and bumps `signin_bump` so the
/// repo picker appears / refetches. Disabled while any sign-in is in flight so a
/// second click can't race a second browser tab against the same store.
#[component]
fn ProviderSignInButton(
    provider: Provider,
    busy: bool,
    signed_in: Signal<Vec<Provider>>,
    signin_bump: Signal<u32>,
    status: Signal<Option<String>>,
    error: Signal<Option<String>>,
) -> Element {
    let style = format!(
        "display:inline-flex;align-items:center;gap:8px;padding:9px 16px;\
         border:none;border-radius:8px;background:{accent};color:{on};\
         font-family:{sans};font-size:13px;font-weight:600;\
         opacity:{opacity};cursor:{cursor};",
        accent = tokens::var::ACCENT,
        on = tokens::var::ON_ACCENT,
        sans = tokens::FONT_SANS,
        opacity = if busy { "0.5" } else { "1" },
        cursor = if busy { "not-allowed" } else { "pointer" },
    );

    // Mutable copies moved into the async task; signals are Copy + 'static.
    let mut status = status;
    let mut error = error;
    let mut signed_in = signed_in;
    let mut signin_bump = signin_bump;

    rsx! {
        button {
            style: "{style}",
            disabled: busy,
            onclick: move |_| {
                // One browser sign-in at a time.
                if busy || status.peek().is_some() {
                    return;
                }
                error.set(None);
                status.set(Some(format!(
                    "Waiting for {} browser sign-in\u{2026}",
                    provider.display_name()
                )));
                spawn(async move {
                    let res = tokio::task::spawn_blocking(move || run_provider_login(provider))
                        .await
                        .unwrap_or_else(|e| Err(format!("internal error: {e}")));
                    match res {
                        Ok(()) => {
                            // Reflect the new credential: refresh the signed-in
                            // set and bump the reload so the picker refetches.
                            signed_in.set(read_signed_in());
                            let n = *signin_bump.peek();
                            signin_bump.set(n.wrapping_add(1));
                            status.set(None);
                        }
                        Err(e) => {
                            status.set(None);
                            error.set(Some(format!(
                                "{} sign-in didn't finish: {e}",
                                provider.display_name()
                            )));
                        }
                    }
                });
            },
            {provider_icon(provider)}
            "Sign in with {provider.display_name()}"
        }
    }
}

/// The searchable list of the operator's permitted repositories. Fetches across
/// every signed-in provider on a background thread, refetching whenever `reload`
/// changes (a new provider signed in). Selecting a repo calls `on_pick` with its
/// clone URL, which the parent feeds into the existing clone + register path.
///
/// Handles the loading, error, empty, and no-match states cleanly: a fetch error
/// surfaces a readable line, never a panic or a hang.
#[component]
pub fn RepoPicker(reload: Signal<u32>, on_pick: EventHandler<String>) -> Element {
    let mut query = use_signal(String::new);
    let mut repos = use_signal(Vec::<Repo>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);

    // (Re)fetch when `reload` bumps. Reading it here establishes the dependency,
    // so a new sign-in re-runs the fetch. The fetch is blocking network I/O, so it
    // runs on spawn_blocking; the UI shows a loading line until it resolves and
    // never blocks the render thread.
    use_effect(move || {
        let _ = reload.read();
        loading.set(true);
        error.set(None);
        spawn(async move {
            let result = tokio::task::spawn_blocking(fetch_permitted_repos)
                .await
                .unwrap_or_else(|e| Err(format!("internal error: {e}")));
            match result {
                Ok(list) => repos.set(list),
                Err(e) => {
                    error.set(Some(e));
                    repos.set(Vec::new());
                }
            }
            loading.set(false);
        });
    });

    let wrap = "display:flex;flex-direction:column;gap:8px;margin-top:12px;";
    let label = format!(
        "font-size:11px;text-transform:uppercase;letter-spacing:0.05em;\
         color:{faint};font-family:{mono};",
        faint = tokens::var::TEXT_FAINT,
        mono = tokens::FONT_MONO,
    );
    let search_wrap = format!(
        "display:flex;align-items:center;gap:7px;padding:6px 9px;\
         border:1px solid {border};border-radius:8px;background:{raised};",
        border = tokens::var::BORDER,
        raised = tokens::var::SURFACE_RAISED,
    );
    let search_input = format!(
        "flex:1;appearance:none;border:0;background:transparent;outline:none;\
         color:{text};font-family:{sans};font-size:12.5px;",
        text = tokens::var::TEXT,
        sans = tokens::FONT_SANS,
    );
    let list_box = format!(
        "max-height:280px;overflow:auto;border:1px solid {border};border-radius:8px;\
         background:{base};",
        border = tokens::var::BORDER,
        base = tokens::var::SURFACE_BASE,
    );
    let note = format!(
        "font-size:11.5px;color:{faint};margin:0;",
        faint = tokens::var::TEXT_FAINT,
    );

    let is_loading = *loading.read();
    let err = error.read().clone();
    let q = query.read().trim().to_ascii_lowercase();
    let all = repos.read();
    let total = all.len();
    let items: Vec<Repo> = all
        .iter()
        .filter(|r| {
            q.is_empty()
                || r.full_name.to_ascii_lowercase().contains(&q)
                || r.name.to_ascii_lowercase().contains(&q)
        })
        .cloned()
        .collect();
    drop(all);

    rsx! {
        div { style: "{wrap}",
            div { style: "{label}", "Your repositories" }
            div { style: "{search_wrap}",
                span { style: format!("color:{};font-size:12px;", tokens::var::TEXT_FAINT), "\u{2315}" }
                input {
                    style: "{search_input}",
                    placeholder: "filter by name or owner/name\u{2026}",
                    value: "{query}",
                    oninput: move |evt| query.set(evt.value()),
                }
            }
            if is_loading {
                p { style: "{note}", "Loading your repositories\u{2026}" }
            } else if let Some(msg) = err {
                p {
                    style: format!(
                        "font-family:{mono};font-size:11.5px;color:{danger};margin:0;",
                        mono = tokens::FONT_MONO,
                        danger = tokens::var::STATUS_DANGER,
                    ),
                    "Couldn't list your repositories: {msg}"
                }
            } else if total == 0 {
                p { style: "{note}",
                    "No repositories found for your signed-in accounts. Paste a Git URL below instead."
                }
            } else if items.is_empty() {
                p { style: "{note}", "Nothing matches that search." }
            } else {
                div { style: "{list_box}",
                    for repo in items.iter() {
                        RepoRow { repo: repo.clone(), on_pick }
                    }
                }
            }
        }
    }
}

/// One selectable repository row: the short name, the owner-qualified path, and a
/// provider tag. Clicking it hands the repo's clone URL to `on_pick`.
#[component]
fn RepoRow(repo: Repo, on_pick: EventHandler<String>) -> Element {
    let row = format!(
        "display:flex;align-items:center;gap:10px;padding:8px 11px;cursor:pointer;\
         border-bottom:1px solid {border};",
        border = tokens::var::BORDER,
    );
    let name = format!(
        "font-family:{sans};font-size:13px;font-weight:600;color:{text};\
         overflow:hidden;text-overflow:ellipsis;white-space:nowrap;",
        sans = tokens::FONT_SANS,
        text = tokens::var::TEXT,
    );
    let full = format!(
        "font-family:{mono};font-size:11px;color:{faint};\
         overflow:hidden;text-overflow:ellipsis;white-space:nowrap;",
        mono = tokens::FONT_MONO,
        faint = tokens::var::TEXT_FAINT,
    );
    let tag = format!(
        "margin-left:auto;flex:none;display:inline-flex;align-items:center;gap:5px;\
         font-family:{mono};font-size:10px;color:{muted};",
        mono = tokens::FONT_MONO,
        muted = tokens::var::TEXT_MUTED,
    );

    let clone_url = clone_url_for(&repo);
    rsx! {
        div {
            style: "{row}",
            role: "button",
            tabindex: "0",
            title: "Clone {repo.full_name}",
            onclick: move |_| on_pick.call(clone_url.clone()),
            div { style: "display:flex;flex-direction:column;gap:2px;min-width:0;",
                span { style: "{name}", "{repo.name}" }
                span { style: "{full}", "{repo.full_name}" }
            }
            span { style: "{tag}",
                {provider_icon(repo.provider)}
                "{repo.provider.display_name()}"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_url_appends_git_suffix_once() {
        let gh = Repo {
            name: "darkrun".into(),
            full_name: "jwaldrip/darkrun".into(),
            url: "https://github.com/jwaldrip/darkrun".into(),
            provider: Provider::GitHub,
        };
        assert_eq!(clone_url_for(&gh), "https://github.com/jwaldrip/darkrun.git");

        // Already `.git` (and a trailing slash) → no double suffix.
        let with_git = Repo {
            url: "https://gitlab.com/g/p.git".into(),
            ..gh.clone()
        };
        assert_eq!(clone_url_for(&with_git), "https://gitlab.com/g/p.git");
        let trailing = Repo {
            url: "https://github.com/o/r/".into(),
            ..gh.clone()
        };
        assert_eq!(clone_url_for(&trailing), "https://github.com/o/r.git");
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;

    fn render(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    #[test]
    fn sign_in_panel_renders_buttons_when_signed_out() {
        // With no signed-in providers, both sign-in buttons render.
        fn App() -> Element {
            let signed_in = use_signal(Vec::<Provider>::new);
            let bump = use_signal(|| 0u32);
            rsx! { SignInPanel { signed_in, signin_bump: bump } }
        }
        let html = render(App);
        assert!(html.contains("Sign in with GitHub"), "{html}");
        assert!(html.contains("Sign in with GitLab"), "{html}");
    }

    #[test]
    fn sign_in_panel_shows_signed_in_state() {
        // A signed-in provider renders its chip instead of a button.
        fn App() -> Element {
            let signed_in = use_signal(|| vec![Provider::GitHub]);
            let bump = use_signal(|| 0u32);
            rsx! { SignInPanel { signed_in, signin_bump: bump } }
        }
        let html = render(App);
        assert!(html.contains("signed in"), "{html}");
        // GitLab still offers its button.
        assert!(html.contains("Sign in with GitLab"), "{html}");
    }

    #[test]
    fn repo_picker_renders_loading_first() {
        // On first render the fetch is pending (the future isn't driven in SSR),
        // so the loading line shows and no network is touched.
        fn App() -> Element {
            let reload = use_signal(|| 0u32);
            rsx! { RepoPicker { reload, on_pick: move |_| {} } }
        }
        let html = render(App);
        assert!(html.contains("Loading your repositories"), "{html}");
    }

    #[test]
    fn repo_row_renders_name_and_provider() {
        fn App() -> Element {
            let repo = Repo {
                name: "darkrun".into(),
                full_name: "jwaldrip/darkrun".into(),
                url: "https://github.com/jwaldrip/darkrun".into(),
                provider: Provider::GitHub,
            };
            rsx! { RepoRow { repo, on_pick: move |_| {} } }
        }
        let html = render(App);
        assert!(html.contains("darkrun") && html.contains("jwaldrip/darkrun"), "{html}");
    }
}
