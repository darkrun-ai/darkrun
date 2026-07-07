//! app.darkrun.ai — the darkrun web app.
//!
//! The remote/fallback surface: when you can't open the desktop or a native
//! mobile app, this Dioxus (wasm) client connects to a live run **through the
//! relay** (see [`remote`]) and renders the review surface from the shared
//! `darkrun-ui` components — the same brand the desktop shows. A smart install
//! banner points at the native apps, which take over via the universal link.

mod banner;
mod firebase;
mod login;
mod register;
mod remote;
mod workspace;

use darkrun_api::session::{
    DirectionSessionPayload, PickerSessionPayload, QuestionSessionPayload, ReviewSessionPayload,
    SessionPayload,
};
use darkrun_ui::prelude::*;
use darkrun_ui::tokens;

use banner::InstallBanner;
use darkrun_api::tunnel::ClientCommand;
use futures::channel::mpsc::UnboundedReceiver;

use login::{LoginPage, StandaloneLogin};
use remote::{run_connection, target_from_url, CommandOutcome, RemoteState};

/// Whether the current page path is the login route. Reads `location.pathname`;
/// the pure predicate is [`is_login_pathname`].
fn is_login_path() -> bool {
    web_sys::window()
        .and_then(|w| w.location().pathname().ok())
        .map(|p| is_login_pathname(&p))
        .unwrap_or(false)
}

/// Whether `path` is the `/login` route (trailing slash tolerated). Pure, so the
/// routing decision is unit-tested off-browser.
fn is_login_pathname(path: &str) -> bool {
    path.trim_end_matches('/').ends_with("/login")
}

/// The review id in the current `/review/:id` path, if that's where we are.
///
/// This is the route a darkrun review link points at — the same
/// `https://app.darkrun.ai/review/<id>` Universal Link the native apps claim
/// (see `desktop/Dioxus.toml [deep_links]`). On the web (the fallback when the
/// native app isn't installed) it renders the review by id below. Reads
/// `location.pathname`; the parsing lives in the pure [`review_id_from_pathname`].
fn review_id_from_path() -> Option<String> {
    let path = web_sys::window()?.location().pathname().ok()?;
    review_id_from_pathname(&path)
}

/// Extract the review id from a `/review/:id` pathname, or `None` when the path
/// isn't a single-segment review route. A trailing slash is tolerated; a nested
/// path (`/review/a/b`) or an empty id (`/review/`) is rejected. Pure (no
/// `web_sys`) so the route parsing is unit-tested on native.
fn review_id_from_pathname(path: &str) -> Option<String> {
    let rest = path.trim_end_matches('/').strip_prefix("/review/")?;
    // Only a single, non-empty segment is a review id.
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Some(rest.to_string())
}

/// The app root: the `/login` page, or the dark shell + install banner + the
/// live session view.
#[component]
pub fn App() -> Element {
    // `/login` is the browser side of `darkrun login` — a distinct flow.
    if is_login_path() {
        return rsx! {
            style { "{tokens::THEME_CSS}" }
            LoginPage {}
        };
    }

    // `/review/:id` — the route a darkrun review link opens (the Universal Link
    // the native apps claim; the web app is the fallback). When the link also
    // carries a live `?relay&session&token` target it reads straight into the
    // live session below (the normal shell handles that); with only the bare
    // path we render the review id with a path back into a live view.
    let review_id = review_id_from_path();

    // The bare app root — no review link, no live run target — is the standalone
    // DASHBOARD (sign in, see your repos + the runs already in them), not the
    // "open a run to watch it live" remote-control landing. That landing is only
    // for an actual run link that can't reach a live host. (Both branches depend
    // only on the URL, which is fixed for the page, so the hook order is stable.)
    if review_id.is_none() && target_from_url().is_none() {
        return rsx! {
            style { "{tokens::THEME_CSS}" }
            StandaloneLogin {}
        };
    }

    let state = use_signal(|| RemoteState::Unconfigured);
    // The outcome of the operator's most recent command, so an approve/answer
    // isn't a silent no-op: the connection reflects the host's ack here.
    let cmd_outcome = use_signal(|| CommandOutcome::Idle);

    // The connection runs as a coroutine so its handle doubles as the command
    // channel: the UI sends a `ClientCommand` (approve a gate, answer a question)
    // and the connection forwards it to the host over the tunnel.
    let commands = use_coroutine(move |cmd_rx: UnboundedReceiver<ClientCommand>| async move {
        if let Some(target) = target_from_url() {
            run_connection(target.url, state, cmd_rx, cmd_outcome).await;
        }
    });

    let shell = format!(
        "min-height:100vh;background:{};color:{};font-family:{};",
        tokens::SURFACE_BASE,
        tokens::TEXT,
        tokens::FONT_SANS,
    );

    rsx! {
        // Mount the shared theme: the `--dr-*` custom properties the darkrun-ui
        // session components resolve against, plus the html/body reset (dark
        // surface, no default margin) — without it the body showed a white frame.
        style { "{tokens::THEME_CSS}" }
        document::Title { "darkrun" }
        div { style: "{shell}",
            Header {}
            InstallBanner {}
            PushPrompt {}
            main { style: "max-width:880px;margin:0 auto;padding:24px 20px 64px;",
                match state() {
                    // On `/review/:id` with no live target, name the review and
                    // point the operator at a live view; otherwise the bare
                    // "open a run" landing.
                    RemoteState::Unconfigured => match &review_id {
                        Some(id) => rsx! { ReviewLanding { id: id.clone() } },
                        None => rsx! { NoTarget {} },
                    },
                    RemoteState::Connecting => rsx! { Status { text: "Connecting to your run\u{2026}" } },
                    RemoteState::Reconnecting => rsx! { Status { text: "Reconnecting\u{2026}" } },
                    RemoteState::Live(payload) => live_view(&payload, commands, cmd_outcome),
                }
            }
        }
    }
}

/// The opt-in state for browser push notifications.
#[derive(Clone, PartialEq)]
enum PushStatus {
    /// Not asked yet.
    Idle,
    /// Permission + registration in flight.
    Asking,
    /// Registered — this browser now receives gate pushes.
    Enabled,
    /// Dismissed for this visit.
    Dismissed,
    /// Failed (denied, unsupported, or the registration POST failed).
    Failed(String),
}

/// A one-line opt-in to remote push, shown only when the app has a connection
/// token (so there's an account to register the device against). It mints an FCM
/// token for this browser and POSTs it to the relay's `/devices`; once enabled
/// or dismissed it disappears. Absent entirely without a token.
#[component]
fn PushPrompt() -> Element {
    let Some(token) = firebase::query_param("token") else {
        return rsx! {};
    };
    let mut status = use_signal(|| PushStatus::Idle);
    if matches!(status(), PushStatus::Enabled | PushStatus::Dismissed) {
        return rsx! {};
    }

    let bar = format!(
        "display:flex;align-items:center;gap:12px;padding:10px 20px;\
         background:{};border-bottom:1px solid {};font-family:{};font-size:13px;",
        tokens::SURFACE_RAISED, tokens::BORDER, tokens::FONT_SANS,
    );
    let asking = matches!(status(), PushStatus::Asking);

    rsx! {
        div { style: "{bar}",
            span { style: format!("flex:1;color:{};", tokens::TEXT),
                "Get notified when a gate needs you — even when this tab is in the background."
            }
            if let PushStatus::Failed(msg) = status() {
                span { style: format!("color:{};", tokens::TEXT_FAINT), "{msg}" }
            }
            button {
                disabled: asking,
                style: format!(
                    "padding:6px 14px;border:none;border-radius:6px;cursor:pointer;\
                     background:{};color:{};font-family:{};font-size:13px;font-weight:600;",
                    tokens::ACCENT, tokens::ON_ACCENT, tokens::FONT_SANS,
                ),
                onclick: move |_| {
                    let token = token.clone();
                    status.set(PushStatus::Asking);
                    spawn(async move {
                        match register::enable_push(&firebase::web_base(), &token).await {
                            Ok(()) => status.set(PushStatus::Enabled),
                            Err(e) => status.set(PushStatus::Failed(e)),
                        }
                    });
                },
                { if asking { "Enabling\u{2026}" } else { "Enable notifications" } }
            }
            button {
                style: format!(
                    "background:none;border:none;cursor:pointer;color:{};font-size:16px;line-height:1;padding:0 4px;",
                    tokens::TEXT_FAINT,
                ),
                onclick: move |_| status.set(PushStatus::Dismissed),
                "\u{2715}"
            }
        }
    }
}

/// The brand header.
#[component]
fn Header() -> Element {
    let bar = format!(
        "display:flex;align-items:center;gap:10px;padding:16px 20px;border-bottom:1px solid {};",
        tokens::BORDER,
    );
    rsx! {
        header { style: "{bar}",
            Wordmark { variant: WordmarkVariant::OutlinedSolidRun, size: 22.0, interactive: true }
            span {
                style: format!(
                    "font-family:{};font-size:12px;color:{};letter-spacing:.06em;text-transform:uppercase;",
                    tokens::FONT_MONO, tokens::TEXT_FAINT,
                ),
                "remote"
            }
        }
    }
}

/// A centered status line (connecting / reconnecting).
#[component]
fn Status(text: String) -> Element {
    rsx! {
        p {
            style: format!(
                "font-family:{};font-size:14px;color:{};text-align:center;padding:48px 0;",
                tokens::FONT_SANS, tokens::TEXT_MUTED,
            ),
            "{text}"
        }
    }
}

/// Shown when the app is opened without a connection target.
#[component]
fn NoTarget() -> Element {
    rsx! {
        div { style: "text-align:center;padding:48px 0;",
            p {
                style: format!(
                    "font-family:{};font-size:16px;color:{};margin:0 0 8px;",
                    tokens::FONT_SANS, tokens::TEXT,
                ),
                "Open a run to watch it live."
            }
            p {
                style: format!(
                    "font-family:{};font-size:13px;color:{};margin:0;",
                    tokens::FONT_SANS, tokens::TEXT_MUTED,
                ),
                "Sign in on your machine with /darkrun:darkrun-login, then open the run's link."
            }
        }
    }
}

/// The `/review/:id` landing shown when the link carried no live relay target.
///
/// The full review link also carries `?relay&session&token`, which drives the
/// live connection (the `Live` view). Opened with only the bare path — e.g. a
/// shared `app.darkrun.ai/review/<id>` link without a token — there's nothing to
/// connect to, so this names the review and points back at a live entry point
/// (the install banner above already offers the native app).
#[component]
fn ReviewLanding(id: String) -> Element {
    rsx! {
        div { style: "text-align:center;padding:48px 0;",
            p {
                style: format!(
                    "font-family:{};font-size:12px;color:{};letter-spacing:.06em;\
                     text-transform:uppercase;margin:0 0 6px;",
                    tokens::FONT_MONO, tokens::TEXT_FAINT,
                ),
                "review"
            }
            p {
                style: format!(
                    "font-family:{};font-size:18px;color:{};margin:0 0 12px;word-break:break-all;",
                    tokens::FONT_SANS, tokens::TEXT,
                ),
                "{id}"
            }
            p {
                style: format!(
                    "font-family:{};font-size:13px;color:{};margin:0;max-width:520px;\
                     margin-left:auto;margin-right:auto;line-height:1.5;",
                    tokens::FONT_SANS, tokens::TEXT_MUTED,
                ),
                "Open this review live from the darkrun app, or sign in on your machine \
                 with /darkrun:darkrun-login and follow the run's link to watch it here."
            }
        }
    }
}

/// Render whatever live session the run is parked on — the checkpoint `Review`,
/// or an interactive `Question` / `Direction` / `Picker` the engine mirrored onto
/// the run feed (so a dark-mode run parked on a question is actionable, not
/// stale). A plain function (not a `#[component]`) because the payload isn't
/// `PartialEq` — it's re-rendered on every live update.
fn live_view(
    payload: &SessionPayload,
    commands: Coroutine<ClientCommand>,
    cmd_outcome: Signal<CommandOutcome>,
) -> Element {
    match payload {
        SessionPayload::Review(p) => session_view(p, commands, cmd_outcome),
        SessionPayload::Question(p) => question_view(p, commands, cmd_outcome),
        SessionPayload::Direction(p) => direction_view(p, commands, cmd_outcome),
        SessionPayload::Picker(p) => picker_view(p, commands, cmd_outcome),
        // `remote::session_payload` never stores the other variants; nothing to
        // render here.
        _ => rsx! {},
    }
}

/// The live review surface for one run: identity, the phase strip, the stations,
/// the station narrative, and — when a gate is open — an Approve action that
/// pushes a command back to the host over the tunnel. `commands` forwards
/// operator actions to the connection; `cmd_outcome` surfaces the host's ack.
fn session_view(
    payload: &ReviewSessionPayload,
    commands: Coroutine<ClientCommand>,
    cmd_outcome: Signal<CommandOutcome>,
) -> Element {
    // A real slug drives the Advance command. A missing/empty slug must NOT send
    // `Advance { run: "run" }` for a literal run named "run" — guard the button.
    let run_slug = guarded_run_slug(&payload.run_slug);
    let run_display = run_slug.clone().unwrap_or_else(|| "run".to_string());
    let phase = active_phase(payload);

    rsx! {
        div { style: "display:flex;flex-direction:column;gap:20px;",
            // Run identity + live phase strip.
            div {
                h1 {
                    style: format!(
                        "font-family:{};font-size:22px;color:{};margin:0 0 10px;",
                        tokens::FONT_SANS, tokens::TEXT,
                    ),
                    "{run_display}"
                }
                StationPipeline { dots: strip_for(phase), labels: true }
            }

            // Gate — what needs the operator now, with the Approve action that
            // advances the run past it (pushed to the host over the tunnel).
            if let Some(gate) = payload.gate_type {
                {
                    let run_slug = run_slug.clone();
                    rsx! {
                        div {
                            style: format!(
                                "display:flex;align-items:center;gap:14px;flex-wrap:wrap;\
                                 padding:14px 16px;border:1px solid {};border-radius:8px;background:{};",
                                tokens::ACCENT_STRONG, tokens::SURFACE_RAISED,
                            ),
                            span {
                                style: format!(
                                    "flex:1;min-width:200px;font-family:{};font-size:14px;color:{};",
                                    tokens::FONT_SANS, tokens::TEXT,
                                ),
                                { format!("A {gate:?} checkpoint is waiting for your decision.") }
                            }
                            match run_slug {
                                Some(run) => rsx! {
                                    button {
                                        style: format!(
                                            "padding:8px 18px;border:none;border-radius:6px;cursor:pointer;\
                                             background:{};color:{};font-family:{};font-size:14px;font-weight:600;",
                                            tokens::ACCENT, tokens::ON_ACCENT, tokens::FONT_SANS,
                                        ),
                                        onclick: move |_| {
                                            let mut cmd_outcome = cmd_outcome;
                                            cmd_outcome.set(CommandOutcome::Pending);
                                            commands.send(ClientCommand::Advance { run: run.clone() });
                                        },
                                        "Approve"
                                    }
                                },
                                None => rsx! {
                                    span {
                                        style: format!(
                                            "font-family:{};font-size:13px;color:{};",
                                            tokens::FONT_SANS, tokens::STATUS_WARN,
                                        ),
                                        "This run's id didn't come through — approve it from the desktop or CLI."
                                    }
                                },
                            }
                        }
                        { command_outcome_note(cmd_outcome) }
                    }
                }
            }

            // The stations and their status.
            div { style: "display:flex;flex-wrap:wrap;gap:8px;",
                for st in payload.station_states.iter() {
                    {
                        let status = st.status.clone().unwrap_or_default();
                        let chip = format!(
                            "display:inline-flex;align-items:center;gap:6px;padding:6px 10px;\
                             border:1px solid {};border-radius:999px;background:{};\
                             font-family:{};font-size:12px;color:{};",
                            tokens::BORDER, tokens::SURFACE_RAISED, tokens::FONT_MONO, tokens::TEXT_MUTED,
                        );
                        rsx! {
                            span { style: "{chip}",
                                span { style: format!("color:{};", tokens::TEXT), "{st.station}" }
                                if !status.is_empty() { span { "{status}" } }
                            }
                        }
                    }
                }
            }

            // Station narrative: what each station produced (the post-outcomes),
            // else what it's about to do (the pre-briefs) — the durable record.
            {
                let narrative = if !payload.station_outcomes.is_empty() {
                    Some(("Outcomes", &payload.station_outcomes))
                } else if !payload.station_briefs.is_empty() {
                    Some(("Briefs", &payload.station_briefs))
                } else {
                    None
                };
                match narrative {
                    Some((label, entries)) => rsx! {
                        div { style: "display:flex;flex-direction:column;gap:12px;",
                            h2 {
                                style: format!(
                                    "font-family:{};font-size:13px;letter-spacing:.06em;text-transform:uppercase;color:{};margin:8px 0 0;",
                                    tokens::FONT_MONO, tokens::TEXT_FAINT,
                                ),
                                "{label}"
                            }
                            for (station, body) in entries.iter() {
                                div {
                                    style: format!(
                                        "padding:12px 14px;border:1px solid {};border-radius:8px;background:{};",
                                        tokens::BORDER, tokens::SURFACE_RAISED,
                                    ),
                                    div {
                                        style: format!(
                                            "font-family:{};font-size:12px;color:{};margin-bottom:6px;",
                                            tokens::FONT_MONO, tokens::ACCENT,
                                        ),
                                        "{station}"
                                    }
                                    p {
                                        style: format!(
                                            "font-family:{};font-size:13px;color:{};margin:0;white-space:pre-wrap;line-height:1.5;",
                                            tokens::FONT_SANS, tokens::TEXT_MUTED,
                                        ),
                                        "{body}"
                                    }
                                }
                            }
                        }
                    },
                    None => rsx! {},
                }
            }
        }
    }
}

/// A visual Question the agent posed mid-run: the prompt plus its options. The
/// operator picks one, which sends the [`ClientCommand::Answer`] the engine
/// resolves the session with. (Single-pick per click covers the common case; the
/// answer shape — `{ "selected": [id] }` — matches the question answer endpoint
/// the tunnel routes to.)
fn question_view(
    payload: &QuestionSessionPayload,
    commands: Coroutine<ClientCommand>,
    cmd_outcome: Signal<CommandOutcome>,
) -> Element {
    let session = payload.session_id.clone();
    let answered = payload.answer.is_some();
    rsx! {
        div { style: "display:flex;flex-direction:column;gap:16px;",
            InteractiveHeader {
                kind: "question".to_string(),
                run: payload.run_slug.clone(),
                title: payload.title.clone(),
                prompt: payload.prompt.clone(),
                context: payload.context.clone(),
            }
            if !payload.image_urls.is_empty() {
                div { style: "display:flex;flex-wrap:wrap;gap:8px;",
                    for url in payload.image_urls.iter() {
                        img {
                            src: "{url}",
                            style: format!("max-width:240px;border-radius:8px;border:1px solid {};", tokens::BORDER),
                        }
                    }
                }
            }
            div { style: "display:grid;grid-template-columns:repeat(auto-fill,minmax(220px,1fr));gap:10px;",
                for opt in payload.options.iter() {
                    {
                        let session = session.clone();
                        let id = opt.id.clone();
                        rsx! {
                            OptionCard {
                                label: opt.label.clone(),
                                description: opt.description.clone(),
                                image_url: opt.image_url.clone(),
                                onpick: move |_| {
                                    let mut cmd_outcome = cmd_outcome;
                                    cmd_outcome.set(CommandOutcome::Pending);
                                    commands.send(ClientCommand::Answer {
                                        session: session.clone(),
                                        answer: serde_json::json!({ "selected": [id.clone()] }),
                                    });
                                },
                            }
                        }
                    }
                }
            }
            if payload.multi_select {
                Hint { text: "This question takes more than one answer; pick the best fit — multi-select from the web is on the way.".to_string() }
            }
            if answered {
                Hint { text: "An answer is already on record; picking again replaces it.".to_string() }
            }
            { command_outcome_note(cmd_outcome) }
        }
    }
}

/// A design Direction the agent asked for: the prompt plus image-backed
/// archetype cards. Picking one sends [`ClientCommand::Answer`] with
/// `{ "archetype": id }`. (Host routing of the Answer command beyond the
/// question endpoint is a later, server-side wave.)
fn direction_view(
    payload: &DirectionSessionPayload,
    commands: Coroutine<ClientCommand>,
    cmd_outcome: Signal<CommandOutcome>,
) -> Element {
    let session = payload.session_id.clone();
    let chosen = payload.chosen_archetype.is_some();
    rsx! {
        div { style: "display:flex;flex-direction:column;gap:16px;",
            InteractiveHeader {
                kind: "direction".to_string(),
                run: payload.run_slug.clone(),
                title: payload.title.clone(),
                prompt: payload.prompt.clone(),
                context: payload.context.clone(),
            }
            div { style: "display:grid;grid-template-columns:repeat(auto-fill,minmax(240px,1fr));gap:10px;",
                for arch in payload.archetypes.iter() {
                    {
                        let session = session.clone();
                        let id = arch.id.clone();
                        rsx! {
                            OptionCard {
                                label: arch.label.clone(),
                                description: Some(arch.description.clone()),
                                image_url: Some(arch.image_url.clone()),
                                onpick: move |_| {
                                    let mut cmd_outcome = cmd_outcome;
                                    cmd_outcome.set(CommandOutcome::Pending);
                                    commands.send(ClientCommand::Answer {
                                        session: session.clone(),
                                        answer: serde_json::json!({ "archetype": id.clone() }),
                                    });
                                },
                            }
                        }
                    }
                }
            }
            if chosen {
                Hint { text: "A direction is already chosen; picking again replaces it.".to_string() }
            }
            { command_outcome_note(cmd_outcome) }
        }
    }
}

/// A blocking Picker the agent raised (factory / mode / size / …): the prompt
/// plus its options. Picking one sends [`ClientCommand::Answer`] with
/// `{ "id": id }`. (As with Direction, host routing of the Answer command beyond
/// the question endpoint is a later, server-side wave.)
fn picker_view(
    payload: &PickerSessionPayload,
    commands: Coroutine<ClientCommand>,
    cmd_outcome: Signal<CommandOutcome>,
) -> Element {
    let session = payload.session_id.clone();
    let selected = payload.selection.is_some();
    rsx! {
        div { style: "display:flex;flex-direction:column;gap:16px;",
            InteractiveHeader {
                kind: "picker".to_string(),
                run: payload.run_slug.clone(),
                title: Some(payload.title.clone()),
                prompt: payload.prompt.clone(),
                context: None,
            }
            div { style: "display:flex;flex-direction:column;gap:8px;",
                for opt in payload.options.iter() {
                    {
                        let session = session.clone();
                        let id = opt.id.clone();
                        rsx! {
                            OptionCard {
                                label: opt.label.clone(),
                                description: opt.description.clone(),
                                image_url: None,
                                onpick: move |_| {
                                    let mut cmd_outcome = cmd_outcome;
                                    cmd_outcome.set(CommandOutcome::Pending);
                                    commands.send(ClientCommand::Answer {
                                        session: session.clone(),
                                        answer: serde_json::json!({ "id": id.clone() }),
                                    });
                                },
                            }
                        }
                    }
                }
            }
            if selected {
                Hint { text: "A choice is already on record; picking again replaces it.".to_string() }
            }
            { command_outcome_note(cmd_outcome) }
        }
    }
}

/// The prompt header shared by the interactive views: a kind tag + optional run
/// slug, an optional title, the prompt, and an optional context preamble.
#[component]
fn InteractiveHeader(
    kind: String,
    run: Option<String>,
    title: Option<String>,
    prompt: String,
    context: Option<String>,
) -> Element {
    let title = title.filter(|t| !t.is_empty());
    let context = context.filter(|c| !c.is_empty());
    rsx! {
        div { style: "display:flex;flex-direction:column;gap:8px;",
            div { style: "display:flex;align-items:center;gap:10px;",
                span {
                    style: format!(
                        "font-family:{};font-size:11px;letter-spacing:.06em;text-transform:uppercase;color:{};",
                        tokens::FONT_MONO, tokens::TEXT_FAINT,
                    ),
                    "{kind}"
                }
                if let Some(run) = run {
                    span {
                        style: format!("font-family:{};font-size:12px;color:{};", tokens::FONT_MONO, tokens::TEXT_MUTED),
                        "{run}"
                    }
                }
            }
            if let Some(title) = title {
                h1 {
                    style: format!("font-family:{};font-size:20px;color:{};margin:0;", tokens::FONT_SANS, tokens::TEXT),
                    "{title}"
                }
            }
            if !prompt.is_empty() {
                p {
                    style: format!("font-family:{};font-size:15px;color:{};margin:0;line-height:1.5;", tokens::FONT_SANS, tokens::TEXT),
                    "{prompt}"
                }
            }
            if let Some(context) = context {
                p {
                    style: format!(
                        "font-family:{};font-size:13px;color:{};margin:0;white-space:pre-wrap;line-height:1.5;",
                        tokens::FONT_SANS, tokens::TEXT_MUTED,
                    ),
                    "{context}"
                }
            }
        }
    }
}

/// One selectable option/archetype card: an optional preview image, a label, and
/// an optional description. `onpick` fires the answer command.
#[component]
fn OptionCard(
    label: String,
    description: Option<String>,
    image_url: Option<String>,
    onpick: EventHandler<MouseEvent>,
) -> Element {
    let description = description.filter(|d| !d.is_empty());
    rsx! {
        button {
            style: format!(
                "display:flex;flex-direction:column;gap:8px;text-align:left;cursor:pointer;\
                 padding:12px 14px;border:1px solid {};border-radius:10px;background:{};",
                tokens::BORDER, tokens::SURFACE_RAISED,
            ),
            onclick: move |e| onpick.call(e),
            if let Some(url) = image_url {
                img {
                    src: "{url}",
                    style: format!(
                        "width:100%;max-height:200px;object-fit:cover;border-radius:6px;border:1px solid {};",
                        tokens::BORDER,
                    ),
                }
            }
            span {
                style: format!("font-family:{};font-size:14px;font-weight:600;color:{};", tokens::FONT_SANS, tokens::TEXT),
                "{label}"
            }
            if let Some(description) = description {
                span {
                    style: format!("font-family:{};font-size:13px;color:{};line-height:1.5;", tokens::FONT_SANS, tokens::TEXT_MUTED),
                    "{description}"
                }
            }
        }
    }
}

/// A small muted hint line.
#[component]
fn Hint(text: String) -> Element {
    rsx! {
        p {
            style: format!("font-family:{};font-size:12px;color:{};margin:0;", tokens::FONT_SANS, tokens::TEXT_FAINT),
            "{text}"
        }
    }
}

/// Surface the most recent command's outcome (sending / applied / failed) so a
/// remote action isn't a silent no-op. Nothing renders until a command is issued.
fn command_outcome_note(cmd_outcome: Signal<CommandOutcome>) -> Element {
    match cmd_outcome() {
        CommandOutcome::Idle => rsx! {},
        CommandOutcome::Pending => rsx! {
            OutcomeNote { text: "Sending your response\u{2026}".to_string(), color: tokens::TEXT_MUTED.to_string() }
        },
        CommandOutcome::Applied => rsx! {
            OutcomeNote { text: "Done — the run has your response.".to_string(), color: tokens::STATUS_OK.to_string() }
        },
        CommandOutcome::Failed(msg) => rsx! {
            OutcomeNote { text: format!("That didn't go through: {msg}"), color: tokens::STATUS_DANGER.to_string() }
        },
    }
}

/// A single-line command-outcome note in `color`.
#[component]
fn OutcomeNote(text: String, color: String) -> Element {
    rsx! {
        p {
            style: format!("font-family:{};font-size:13px;color:{};margin:0;", tokens::FONT_SANS, color),
            "{text}"
        }
    }
}

/// The active phase as a `darkrun-ui` [`Phase`], from the payload's current state.
fn active_phase(payload: &ReviewSessionPayload) -> Option<Phase> {
    let rp = payload.current_state.as_ref()?.phase.as_ref()?;
    let name = serde_json::to_value(rp).ok()?;
    Phase::from_name(name.as_str()?)
}

/// The run slug to drive the Approve/Advance command with: the payload's slug,
/// but only when it's a real, non-empty value. A missing/empty slug returns
/// `None` so the UI never sends `Advance { run: "run" }` for a literal run named
/// "run" (it shows the "approve from desktop/CLI" fallback instead).
fn guarded_run_slug(run_slug: &Option<String>) -> Option<String> {
    run_slug.clone().filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    //! Native (`#[test]`) coverage of the pure routing + session-render helpers:
    //! `/login` and `/review/:id` path parsing, the active-phase mapping, and the
    //! run-slug guard. None touch `web_sys`, so they run under
    //! `cargo test -p darkrun-app`.
    use super::*;
    use darkrun_api::session::{RunCurrentState, RunPhase};

    #[test]
    fn is_login_pathname_matches_the_login_route() {
        assert!(is_login_pathname("/login"));
        assert!(is_login_pathname("/login/")); // trailing slash tolerated
        assert!(!is_login_pathname("/"));
        assert!(!is_login_pathname("/review/abc"));
        // Only a `/login` SEGMENT — a slug that merely ends in "login" must match
        // the segment boundary; `ends_with("/login")` requires the slash.
        assert!(!is_login_pathname("/relogin"));
    }

    #[test]
    fn review_id_from_pathname_reads_a_single_segment_id() {
        assert_eq!(review_id_from_pathname("/review/abc123"), Some("abc123".into()));
        // Trailing slash tolerated.
        assert_eq!(review_id_from_pathname("/review/abc123/"), Some("abc123".into()));
    }

    #[test]
    fn review_id_from_pathname_rejects_non_review_and_nested_paths() {
        assert_eq!(review_id_from_pathname("/"), None);
        assert_eq!(review_id_from_pathname("/login"), None);
        assert_eq!(review_id_from_pathname("/review/"), None); // empty id
        assert_eq!(review_id_from_pathname("/review"), None); // no segment
        // A nested path is not a bare review id.
        assert_eq!(review_id_from_pathname("/review/a/b"), None);
    }

    #[test]
    fn active_phase_maps_the_current_state_phase() {
        let payload = ReviewSessionPayload {
            current_state: Some(RunCurrentState {
                phase: Some(RunPhase::Manufacture),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(active_phase(&payload), Some(Phase::Manufacture));

        // Every RunPhase resolves to its Phase counterpart (the wire names match).
        for (rp, expect) in [
            (RunPhase::Spec, Phase::Spec),
            (RunPhase::Review, Phase::Review),
            (RunPhase::Manufacture, Phase::Manufacture),
            (RunPhase::Audit, Phase::Audit),
            (RunPhase::Reflect, Phase::Reflect),
            (RunPhase::Checkpoint, Phase::Checkpoint),
        ] {
            let p = ReviewSessionPayload {
                current_state: Some(RunCurrentState { phase: Some(rp), ..Default::default() }),
                ..Default::default()
            };
            assert_eq!(active_phase(&p), Some(expect));
        }
    }

    #[test]
    fn active_phase_is_none_without_a_current_state_or_phase() {
        assert_eq!(active_phase(&ReviewSessionPayload::default()), None);
        let no_phase = ReviewSessionPayload {
            current_state: Some(RunCurrentState { phase: None, ..Default::default() }),
            ..Default::default()
        };
        assert_eq!(active_phase(&no_phase), None);
    }

    #[test]
    fn guarded_run_slug_keeps_real_slugs_and_drops_empty_ones() {
        assert_eq!(guarded_run_slug(&Some("my-run".into())), Some("my-run".into()));
        assert_eq!(guarded_run_slug(&None), None);
        // An empty slug is guarded out so no `Advance { run: "run" }` is sent.
        assert_eq!(guarded_run_slug(&Some(String::new())), None);
    }
}
