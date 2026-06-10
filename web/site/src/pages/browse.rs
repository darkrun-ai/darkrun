//! `/browse` — the web viewer for a *published / remote* workspace.
//!
//! The website browses runs read-only: point it at a repository and it renders
//! that workspace's runs, stations, units, and artifacts in the browser. There
//! is deliberately **no local-folder picker here** — picking and opening a local
//! workspace (and reviewing it) is the **desktop app's** job. The agent never
//! opens a browser; the desktop app is the only interactive surface it drives.
//!
//! Two surfaces live here:
//! - the bare `/browse` landing: paste a repo to open its run list;
//! - the deep `/browse/{host}/{owner}/{repo}[/run/{slug}]` views, which fetch the
//!   repo's `.darkrun/` workspace over CORS-enabled HTTP (see [`crate::remote`])
//!   and render it with the shared `darkrun-ui` components — the same station
//!   strip, phase pipeline, and unit DAG the desktop app draws.

use darkrun_ui::prelude::*;

use crate::auth;
use crate::remote::{self, RepoRef, RunDetail, Target};
use crate::route::Route;
use crate::ui::theme;
use crate::ui::SectionHead;

/// The OAuth/token provider key for a host (`github` / `gitlab`).
fn provider_key(host: &str) -> &'static str {
    if host.contains("gitlab") {
        "gitlab"
    } else {
        "github"
    }
}

/// The display name for a host's provider.
fn provider_name(host: &str) -> &'static str {
    if host.contains("gitlab") {
        "GitLab"
    } else {
        "GitHub"
    }
}

/// Whether the host's GraphQL REQUIRES a token. GitHub forbids anonymous GraphQL;
/// GitLab reads public projects anonymously (a token only unlocks private ones).
fn host_requires_token(host: &str) -> bool {
    provider_key(host) == "github"
}

/// A small GitHub / GitLab brand mark for `host`, in the current text color.
#[component]
fn HostIcon(host: String, #[props(default = 15)] size: u32) -> Element {
    let gitlab = host.contains("gitlab");
    // simple-icons single-path marks (viewBox 0 0 24 24).
    let d = if gitlab {
        "M23.6004 9.5927l-.0337-.0862L20.3.9814a.851.851 0 00-.3362-.405.8748.8748 0 00-.9997.0539.8748.8748 0 00-.29.4399l-2.2055 6.748H7.5375l-2.2057-6.748a.8573.8573 0 00-.29-.4412.8748.8748 0 00-.9997-.0537.8585.8585 0 00-.3362.4049L.4332 9.5015l-.0325.0862a6.0657 6.0657 0 002.0119 7.0105l.0113.0087.03.0213 4.976 3.7264 2.462 1.8633 1.4995 1.1321a1.0085 1.0085 0 001.2197 0l1.4995-1.1321 2.462-1.8633 5.006-3.7489.0125-.01a6.0682 6.0682 0 002.0094-7.003z"
    } else {
        "M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12"
    };
    let label = if gitlab { "GitLab" } else { "GitHub" };
    rsx! {
        svg {
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "currentColor",
            role: "img",
            "aria-label": label,
            path { d }
        }
    }
}

/// `/browse` — view a published/remote darkrun workspace in the browser.
#[component]
pub fn Browse() -> Element {
    rsx! {
        SectionHead {
            kicker: "browse a workspace".to_string(),
            title: "Browse a published workspace".to_string(),
            lead: Some(
                "View a darkrun workspace's runs, stations, units, and artifacts read-only, \
                 right in your browser. Point at a repository to render its published workspace."
                    .to_string(),
            ),
        }

        RemoteRepo {}

        RecentlyBrowsed {}

        DesktopForLocal {}
    }
}

/// The local browse history — the repositories you've opened, most-recent first
/// (stored in this browser, never on a server). Empty until you've browsed one.
#[component]
fn RecentlyBrowsed() -> Element {
    let mut repos = use_signal(Vec::<RepoRef>::new);
    use_future(move || async move {
        repos.set(crate::history::recent().await);
    });
    let nav = use_navigator();

    let list = repos.read().clone();
    if list.is_empty() {
        return rsx! {};
    }

    let row = format!(
        "display:flex;align-items:center;justify-content:space-between;gap:10px;\
         padding:9px 14px;border:1px solid {border};border-radius:8px;background:{raised};",
        border = theme::BORDER,
        raised = theme::SURFACE_RAISED,
    );
    rsx! {
        div { style: "margin-top:26px;",
            div {
                style: format!(
                    "font-family:{};font-size:11px;text-transform:uppercase;letter-spacing:0.06em;color:{};margin-bottom:10px;",
                    tokens::FONT_MONO, theme::TEXT_FAINT,
                ),
                "recently browsed"
            }
            div { style: "display:flex;flex-direction:column;gap:8px;",
                for repo in list.iter() {
                    {
                        let go = repo.clone();
                        let drop = repo.clone();
                        let mut repos_sig = repos;
                        rsx! {
                            div { style: "{row}",
                                button {
                                    style: format!(
                                        "flex:1;text-align:left;background:transparent;border:none;cursor:pointer;\
                                         font-family:{};font-size:14px;color:{};padding:0;",
                                        tokens::FONT_MONO, theme::TEXT,
                                    ),
                                    onclick: move |_| { nav.push(Route::BrowseTarget { rest: go.list_rest() }); },
                                    span { style: format!("color:{};", theme::TEXT_FAINT), "{repo.host}/" }
                                    "{repo.slug()}"
                                }
                                span { style: format!("color:{};display:inline-flex;", theme::TEXT_FAINT),
                                    HostIcon { host: repo.host.clone(), size: 15 }
                                }
                                button {
                                    style: format!(
                                        "background:transparent;border:none;cursor:pointer;color:{};font-size:16px;line-height:1;padding:2px 6px;",
                                        theme::TEXT_FAINT,
                                    ),
                                    title: "forget",
                                    onclick: move |_| {
                                        crate::history::forget(&drop);
                                        repos_sig.write().retain(|r| r != &drop);
                                    },
                                    "\u{00d7}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The remote-repository entry: type a repo URL to open its published workspace.
/// The button parses the input into `/browse/{host}/{owner}/{repo}` and routes
/// there — the deep view does the fetching.
#[component]
fn RemoteRepo() -> Element {
    let mut input = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let nav = use_navigator();

    // Parse the typed repo and route to its run list, or flag a bad reference.
    let mut go = move || {
        let raw = input.read().clone();
        match remote::normalize_repo_input(&raw) {
            Some(segs) => {
                error.set(None);
                nav.push(Route::BrowseTarget { rest: segs });
            }
            None => error.set(Some(
                "Enter a repository like github.com/org/repo.".to_string(),
            )),
        }
    };

    let card = format!(
        "border:1px solid {border};border-radius:12px;padding:18px 20px;background:{raised};",
        border = theme::BORDER,
        raised = theme::SURFACE_RAISED,
    );
    let input_style = format!(
        "flex:1;font-family:{mono};font-size:13px;color:{text};background:{surface};\
         border:1px solid {border};border-radius:8px;padding:9px 12px;",
        mono = tokens::FONT_MONO,
        text = theme::TEXT,
        surface = theme::SURFACE_BASE,
        border = theme::BORDER,
    );
    let btn = format!(
        "font-family:{sans};font-size:13px;font-weight:600;color:{on};background:{accent};\
         border:none;border-radius:8px;padding:9px 16px;cursor:pointer;",
        sans = tokens::FONT_SANS,
        on = theme::ON_ACCENT,
        accent = theme::ACCENT,
    );
    rsx! {
        div { style: "{card}",
            div {
                style: format!(
                    "font-family:{};font-size:11px;text-transform:uppercase;letter-spacing:0.06em;color:{};margin-bottom:10px;",
                    tokens::FONT_MONO, theme::TEXT_FAINT,
                ),
                "browse a remote repository"
            }
            div { style: "display:flex;gap:10px;align-items:center;",
                input {
                    style: "{input_style}",
                    r#type: "text",
                    placeholder: "github.com/org/repo or gitlab.com/group/project",
                    value: "{input}",
                    oninput: move |e| input.set(e.value()),
                    onkeydown: move |e| if e.key() == Key::Enter { go() },
                    "data-darkrun-remote": "true",
                }
                button {
                    style: "{btn}",
                    onclick: move |_| go(),
                    "data-darkrun-remote-go": "true",
                    "Browse"
                }
            }
            if let Some(msg) = error.read().as_ref() {
                p {
                    style: format!(
                        "font-family:{};font-size:12px;color:{};margin:10px 0 0;",
                        tokens::FONT_SANS, theme::STATUS_WARN,
                    ),
                    "{msg}"
                }
            }
            p {
                style: format!("font-family:{};font-size:13px;color:{};margin:12px 0 0;", tokens::FONT_SANS, theme::TEXT_MUTED),
                "Renders the repo's "
                code {
                    style: format!("font-family:{};color:{};", tokens::FONT_MONO, theme::ACCENT),
                    ".darkrun/"
                }
                " workspace read-only \u{2014} a shareable link to a run's shape. Each run is read \
                 from its own "
                code {
                    style: format!("font-family:{};color:{};", tokens::FONT_MONO, theme::ACCENT),
                    "darkrun/<run>/main"
                }
                " branch. Public GitHub or GitLab repositories, no sign-in."
            }
        }
    }
}

/// A note pointing local browsing and review at the desktop app.
#[component]
fn DesktopForLocal() -> Element {
    let wrap = format!(
        "margin-top:18px;border:1px solid {border};border-left:3px solid {accent};border-radius:8px;\
         padding:12px 16px;background:{overlay};",
        border = theme::BORDER,
        accent = theme::ACCENT,
        overlay = theme::SURFACE_OVERLAY,
    );
    rsx! {
        div { style: "{wrap}",
            div { style: "display:flex;align-items:center;gap:8px;margin-bottom:8px;",
                Badge { tone: Tone::Accent, filled: true, "desktop app" }
                Badge { tone: Tone::Neutral, "your local runs" }
            }
            p {
                style: format!("font-family:{};font-size:13px;color:{};margin:0;", tokens::FONT_SANS, theme::TEXT_MUTED),
                "To browse and review your own runs, open the darkrun desktop app \u{2014} run "
                code {
                    style: format!("font-family:{};color:{};", tokens::FONT_MONO, theme::ACCENT),
                    "darkrun serve"
                }
                ". It picks your local workspace and opens any run into its live review on your \
                 machine. The web browse here is read-only and never touches your local files."
            }
        }
    }
}

// ───────────────────────── deep views ──────────────────────────────────────

/// `/browse/{host}/{owner}/{repo}[/run/{slug}]` — dispatch to the run list or a
/// single run's detail, parsed from the catch-all segments.
#[component]
pub fn BrowseTarget(rest: Vec<String>) -> Element {
    match Target::parse(&rest) {
        Some(Target::RunList(repo)) => rsx! { RunListView { repo } },
        Some(Target::Run(repo, slug)) => rsx! { RunDetailView { repo, slug } },
        None => rsx! {
            SectionHead {
                kicker: "browse".to_string(),
                title: "Not a workspace address".to_string(),
                lead: Some(
                    "A browse link looks like /browse/github.com/org/repo. \
                     Start from the browse page and paste a repository."
                        .to_string(),
                ),
            }
            BackToBrowse {}
        },
    }
}

/// A "← browse" link back to the bare landing.
#[component]
fn BackToBrowse() -> Element {
    rsx! {
        Link {
            to: Route::Browse {},
            class: "dr-navlink",
            style: format!(
                "font-family:{};font-size:13px;color:{};text-decoration:none;",
                tokens::FONT_MONO, theme::ACCENT,
            ),
            "\u{2190} browse another repository"
        }
    }
}

/// A muted status line (loading / empty / error) shared by the deep views.
#[component]
fn StatusLine(text: String, warn: bool) -> Element {
    let color = if warn { theme::STATUS_WARN } else { theme::TEXT_MUTED };
    rsx! {
        div {
            style: format!(
                "border:1px solid {border};border-radius:10px;padding:16px 18px;background:{raised};\
                 font-family:{sans};font-size:14px;color:{color};",
                border = theme::BORDER,
                raised = theme::SURFACE_RAISED,
                sans = tokens::FONT_SANS,
            ),
            "{text}"
        }
    }
}

/// The run list for a repository: a grid of run cards, each opening that run.
#[component]
fn RunListView(repo: RepoRef) -> Element {
    let token = use_browse_token(&repo.host);
    let mut data = use_signal(|| Option::<Result<Vec<RunCardData>, String>>::None);
    let load_repo = repo.clone();
    // `use_effect` (NOT `use_future`) so the fetch re-runs when the token signal
    // resolves from localStorage or a sign-in lands one. `use_future` fires once,
    // before the token loads, and would hang on "Reading…" forever.
    use_effect(move || {
        let tok = token.read().clone();
        let repo = load_repo.clone();
        spawn(async move {
            let Some(tok) = tok else { return };
            data.set(None);
            let r = remote::fetch_run_list(&repo, tok.as_deref())
                .await
                .map_err(|e| e.to_string());
            data.set(Some(r));
        });
    });

    // Remember this repo for the browse history (most-recent first).
    let record_repo = repo.clone();
    use_effect(move || crate::history::record(&record_repo));

    let nav = use_navigator();
    let select_repo = repo.clone();

    rsx! {
        SectionHead {
            kicker: "published workspace".to_string(),
            title: repo.slug(),
            lead: Some(format!("Runs published under .darkrun/ in {}.", repo.host)),
            trailing: rsx! {
                span { style: format!("color:{};", theme::TEXT_MUTED),
                    HostIcon { host: repo.host.clone(), size: 22 }
                }
            },
        }
        BackToBrowse {}
        ConnectBar { host: repo.host.clone(), token }
        div { style: "margin-top:18px;",
            match &*data.read() {
                None => rsx! { StatusLine { text: "Reading the workspace\u{2026}".to_string(), warn: false } },
                Some(Err(e)) => rsx! { StatusLine { text: e.clone(), warn: true } },
                Some(Ok(runs)) if runs.is_empty() => rsx! {
                    StatusLine { text: "No runs in this workspace yet.".to_string(), warn: false }
                },
                Some(Ok(runs)) => rsx! {
                    RunList {
                        runs: runs.clone(),
                        on_select: move |slug: String| {
                            nav.push(Route::BrowseTarget { rest: select_repo.run_rest(&slug) });
                        },
                    }
                },
            }
        }
    }
}

/// A token signal seeded from localStorage once on mount. `None` means "not yet
/// loaded" (the views wait for it); `Some(None)` means "loaded, anonymous".
fn use_browse_token(host: &str) -> Signal<Option<Option<String>>> {
    let provider = provider_key(host).to_string();
    let mut token = use_signal(|| Option::<Option<String>>::None);
    use_future(move || {
        let provider = provider.clone();
        async move {
            let t = auth::stored_token(&provider).await;
            token.set(Some(t));
        }
    });
    token
}

/// The sign-in control for hosts whose GraphQL needs a token (GitHub). Connecting
/// runs the OAuth popup (the existing CLI broker), stores the token client-side,
/// and flips browse onto the batched GraphQL path. GitLab reads anonymously, so
/// no bar shows for it.
#[component]
fn ConnectBar(host: String, token: Signal<Option<Option<String>>>) -> Element {
    let provider = provider_key(&host).to_string();
    let name = provider_name(&host);
    let required = host_requires_token(&host);
    let connected = matches!(&*token.read(), Some(Some(_)));
    let loaded = token.read().is_some();

    let connect_provider = provider.clone();
    let on_connect = move |_| {
        let provider = connect_provider.clone();
        let mut token = token;
        spawn(async move {
            if let Some(t) = auth::connect(&provider).await {
                token.set(Some(Some(t)));
            }
        });
    };
    let disconnect_provider = provider.clone();
    let on_disconnect = move |_| {
        auth::clear_token(&disconnect_provider);
        let mut token = token;
        token.set(Some(None));
    };

    // Paste-a-token (the predecessor's local-testing path): store a PAT directly,
    // no OAuth round-trip — handy when serving the site locally. `use_callback`
    // is `Copy`, so both the button and Enter can fire it.
    let pasted = use_signal(String::new);
    let paste_provider = provider.clone();
    let on_paste = use_callback(move |_: ()| {
        let t = pasted.read().trim().to_string();
        if !t.is_empty() {
            auth::store_token(&paste_provider, &t);
            let mut token = token;
            token.set(Some(Some(t)));
            let mut pasted = pasted;
            pasted.set(String::new());
        }
    });

    let note = format!(
        "font-family:{};font-size:13px;color:{};",
        tokens::FONT_SANS, theme::TEXT_MUTED,
    );
    let btn = format!(
        "font-family:{sans};font-size:12px;font-weight:600;color:{on};background:{accent};\
         border:none;border-radius:6px;padding:6px 12px;cursor:pointer;",
        sans = tokens::FONT_SANS,
        on = theme::ON_ACCENT,
        accent = theme::ACCENT,
    );
    let ghost = format!(
        "font-family:{mono};font-size:12px;color:{muted};background:transparent;\
         border:1px solid {border};border-radius:6px;padding:5px 10px;cursor:pointer;",
        mono = tokens::FONT_MONO,
        muted = theme::TEXT_MUTED,
        border = theme::BORDER,
    );
    let token_input = format!(
        "flex:1;min-width:200px;font-family:{mono};font-size:12px;color:{text};background:{surface};\
         border:1px solid {border};border-radius:6px;padding:5px 8px;",
        mono = tokens::FONT_MONO,
        text = theme::TEXT,
        surface = theme::SURFACE_BASE,
        border = theme::BORDER,
    );

    if !loaded {
        return rsx! {};
    }
    let wrap_col = format!(
        "margin-top:14px;border:1px solid {border};border-radius:8px;padding:10px 14px;\
         background:{raised};display:flex;flex-direction:column;gap:10px;",
        border = theme::BORDER,
        raised = theme::SURFACE_RAISED,
    );
    rsx! {
        div { style: "{wrap_col}",
            div { style: "display:flex;align-items:center;gap:10px;flex-wrap:wrap;",
                if connected {
                    Badge { tone: Tone::Ok, filled: true, "GraphQL" }
                    span { style: "{note}", "Signed in to {name} \u{2014} the whole workspace in one query per run." }
                    button { style: "{ghost}", onclick: on_disconnect, "sign out" }
                } else if required {
                    Badge { tone: Tone::Warn, filled: true, "sign in" }
                    span { style: "{note}",
                        "{name} has no anonymous API \u{2014} connect to browse. Your token stays in \
                         this browser."
                    }
                    button { style: "{btn}", onclick: on_connect, "Connect {name}" }
                } else {
                    Badge { tone: Tone::Accent, filled: true, "GraphQL" }
                    span { style: "{note}",
                        "Browsing public projects anonymously over GraphQL. Connect {name} for \
                         private projects."
                    }
                    button { style: "{btn}", onclick: on_connect, "Connect {name}" }
                }
            }
            // Paste-a-token row (local testing) — shown until a token is set.
            if !connected {
                div { style: "display:flex;align-items:center;gap:8px;flex-wrap:wrap;",
                    span {
                        style: format!("font-family:{};font-size:11px;color:{};", tokens::FONT_MONO, theme::TEXT_FAINT),
                        "or paste a {name} token:"
                    }
                    input {
                        style: "{token_input}",
                        r#type: "password",
                        placeholder: "glpat-… / ghp_… (stays in this browser)",
                        value: "{pasted}",
                        oninput: move |e| { let mut pasted = pasted; pasted.set(e.value()); },
                        onkeydown: move |e| if e.key() == Key::Enter { on_paste.call(()) },
                    }
                    button { style: "{ghost}", onclick: move |_| on_paste.call(()), "use token" }
                }
            }
        }
    }
}

/// One run's detail: identity + the assembly line, the active phase pipeline, the
/// run document, the unit DAG, and feedback — all derived from the remote
/// workspace and drawn with the shared components.
#[component]
fn RunDetailView(repo: RepoRef, slug: String) -> Element {
    let token = use_browse_token(&repo.host);
    let mut data = use_signal(|| Option::<Result<RunDetail, String>>::None);
    let load_repo = repo.clone();
    let load_slug = slug.clone();
    use_effect(move || {
        let tok = token.read().clone();
        let repo = load_repo.clone();
        let slug = load_slug.clone();
        spawn(async move {
            let Some(tok) = tok else { return };
            data.set(None);
            let r = remote::fetch_run_detail(&repo, &slug, tok.as_deref())
                .await
                .map_err(|e| e.to_string());
            data.set(Some(r));
        });
    });

    rsx! {
        SectionHead {
            kicker: format!("{} \u{00b7} run", repo.slug()),
            title: slug.clone(),
            lead: None,
        }
        ConnectBar { host: repo.host.clone(), token }
        Link {
            to: Route::BrowseTarget { rest: repo.list_rest() },
            class: "dr-navlink",
            style: format!(
                "font-family:{};font-size:13px;color:{};text-decoration:none;",
                tokens::FONT_MONO, theme::ACCENT,
            ),
            "\u{2190} all runs in {repo.slug()}"
        }
        div { style: "margin-top:18px;",
            match &*data.read() {
                None => rsx! { StatusLine { text: "Reading the run\u{2026}".to_string(), warn: false } },
                Some(Err(e)) => rsx! { StatusLine { text: e.clone(), warn: true } },
                Some(Ok(detail)) => run_detail_body(detail),
            }
        }
    }
}

/// Render a fully-loaded [`RunDetail`].
fn run_detail_body(d: &RunDetail) -> Element {
    let stations: Vec<StationItem> = d
        .stations
        .iter()
        .map(|s| StationItem { name: s.name.clone(), status: s.status, has_feedback: false })
        .collect();

    let h2 = format!(
        "font-family:{};font-size:18px;color:{};margin:28px 0 12px;",
        tokens::FONT_SANS, theme::TEXT,
    );

    rsx! {
        // Identity badges.
        div { style: "display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin-bottom:6px;",
            Badge { tone: Tone::Neutral, "{d.factory}" }
            Badge { tone: Tone::Info, "{d.mode}" }
            Badge { tone: run_status_tone(&d.status), filled: true, "{d.status}" }
            span {
                style: format!("font-family:{};font-size:12px;color:{};", tokens::FONT_MONO, theme::TEXT_FAINT),
                "branch {d.source_ref}"
            }
            if let Some(started) = d.started_at.as_ref() {
                span {
                    style: format!("font-family:{};font-size:12px;color:{};", tokens::FONT_MONO, theme::TEXT_FAINT),
                    "started {started}"
                }
            }
        }

        // The assembly line + the active station's phase pipeline.
        if !stations.is_empty() {
            div { style: "margin:14px 0 4px;", StationStrip { stations } }
            div { style: "display:flex;justify-content:center;margin-bottom:8px;",
                StationPipeline { dots: strip_for(d.active_phase), size: 15.0 }
            }
        }

        // The run document.
        if !d.body_html.trim().is_empty() {
            article { class: "dr-prose", style: "margin-top:18px;", dangerous_inner_html: "{d.body_html}" }
        }

        // The unit dependency graph + rows.
        if d.has_units() {
            h2 { style: "{h2}", "Units" }
            div { style: "overflow-x:auto;margin-bottom:14px;",
                UnitGraph { units: d.graph_nodes(), edges: d.graph_edges() }
            }
            div { style: "display:flex;flex-direction:column;gap:8px;",
                for u in d.units.iter() {
                    UnitRow {
                        title: u.title.clone(),
                        unit_type: (!u.unit_type.is_empty()).then(|| u.unit_type.clone()),
                        status: u.tone,
                        status_label: u.status.clone(),
                        pass: u.passes,
                    }
                }
            }
        }

        // Feedback.
        if !d.feedback.is_empty() {
            h2 { style: "{h2}", "Feedback" }
            div { style: "display:flex;flex-direction:column;gap:8px;",
                for f in d.feedback.iter() {
                    FeedbackRow {
                        id: f.id.clone(),
                        title: f.title.clone(),
                        status: f.status.clone(),
                        severity: f.severity.clone(),
                        author: f.author.clone(),
                    }
                }
            }
        }
    }
}

/// One feedback item row.
#[component]
fn FeedbackRow(
    id: String,
    title: String,
    status: String,
    severity: Option<String>,
    author: String,
) -> Element {
    let sev_tone = match severity.as_deref() {
        Some("blocker") => Tone::Danger,
        Some("high") => Tone::Warn,
        Some("medium") => Tone::Info,
        _ => Tone::Neutral,
    };
    let row = format!(
        "display:flex;align-items:center;gap:10px;padding:8px 12px;\
         border:1px solid {border};border-radius:8px;background:{raised};",
        border = theme::BORDER,
        raised = theme::SURFACE_RAISED,
    );
    rsx! {
        div { style: "{row}",
            code {
                style: format!("font-family:{};font-size:12px;color:{};min-width:56px;", tokens::FONT_MONO, theme::TEXT_FAINT),
                "{id}"
            }
            if let Some(sev) = severity.as_ref() {
                Badge { tone: sev_tone, filled: true, "{sev}" }
            }
            span {
                style: format!("font-family:{};font-size:13px;color:{};flex:1;", tokens::FONT_SANS, theme::TEXT),
                if title.is_empty() { "(untitled)" } else { "{title}" }
            }
            if !status.is_empty() {
                Badge { tone: Tone::Neutral, "{status}" }
            }
            if !author.is_empty() {
                span {
                    style: format!("font-family:{};font-size:12px;color:{};", tokens::FONT_MONO, theme::TEXT_FAINT),
                    "{author}"
                }
            }
        }
    }
}
