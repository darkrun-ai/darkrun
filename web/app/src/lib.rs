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
mod runs;
mod workspace;

use darkrun_api::session::{
    DirectionSessionPayload, PickerSessionPayload, ProofSessionPayload, QuestionSessionPayload,
    ReviewSessionPayload, SessionPayload, ViewSessionPayload, VisualReviewSessionPayload,
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
/// `location.pathname`; the parsing lives in the pure [`single_segment_after_in`].
fn review_id_from_path() -> Option<String> {
    single_segment_after("/review/")
}

/// The run slug in the current `/runs/:slug` path, if that's where we are.
///
/// This is the CLEAN run link the engine mints (`run_web_url` →
/// `https://app.darkrun.ai/runs/<slug>`): no secret in the URL. The
/// [`runs::RunView`] below resolves the relay/session/token from an
/// authenticated API and drives the live surface.
fn run_slug_from_path() -> Option<String> {
    single_segment_after("/runs/")
}

/// The single, non-empty path segment right after `prefix` in the current URL
/// path (e.g. the id in `/review/<id>` or the slug in `/runs/<slug>`), or `None`
/// when the path isn't that shape.
fn single_segment_after(prefix: &str) -> Option<String> {
    let path = web_sys::window()?.location().pathname().ok()?;
    single_segment_after_in(&path, prefix)
}

/// The single, non-empty path segment right after `prefix` in `path` (e.g. the
/// id in `/review/<id>` or the slug in `/runs/<slug>`). A trailing slash is
/// tolerated; a nested path (`/review/a/b`) or an empty segment (`/review/`) is
/// rejected. Pure (no `web_sys`) so the route parsing is unit-tested on native.
fn single_segment_after_in(path: &str, prefix: &str) -> Option<String> {
    let rest = path.trim_end_matches('/').strip_prefix(prefix)?;
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

    // `/runs/:slug` — the CLEAN run link the engine mints. No secret in the URL:
    // [`runs::RunView`] restores the persisted Firebase token, resolves the relay
    // descriptor from an authenticated API, and drives the live surface (or a
    // clear "not live" state). A distinct branch so the hook order stays stable.
    if let Some(slug) = run_slug_from_path() {
        return rsx! {
            style { "{tokens::THEME_CSS}" }
            runs::RunView { slug }
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
        // The scoped `.dr-md` rules so agent-authored prose (station narrative,
        // interactive prompts) renders as formatted markdown, matching the desktop.
        style { "{darkrun_ui::markdown::CSS}" }
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
                    RemoteState::Offline => rsx! { Offline {} },
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
pub(crate) fn Header() -> Element {
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
pub(crate) fn Status(text: String) -> Element {
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

/// The terminal "connection lost" surface, shown when the relay retry budget is
/// spent (see [`remote::run_connection`]). Distinct from the transient
/// `Reconnecting` status: the connection loop has GIVEN UP, so this states that
/// plainly and offers an explicit retry (a full reload re-runs the whole connect
/// flow) instead of leaving the operator on a silent spinner forever.
#[component]
pub(crate) fn Offline() -> Element {
    rsx! {
        div { style: "text-align:center;padding:48px 0;",
            p {
                style: format!(
                    "font-family:{};font-size:16px;color:{};margin:0 0 8px;",
                    tokens::FONT_SANS, tokens::TEXT,
                ),
                "Connection lost."
            }
            p {
                style: format!(
                    "font-family:{};font-size:13px;color:{};margin:0 0 20px;",
                    tokens::FONT_SANS, tokens::TEXT_MUTED,
                ),
                "We couldn't reach your run after several tries. It may have finished, \
                 or the machine running it went offline."
            }
            button {
                style: format!(
                    "padding:8px 18px;border:none;border-radius:6px;cursor:pointer;\
                     background:{};color:{};font-family:{};font-size:14px;font-weight:600;",
                    tokens::ACCENT, tokens::ON_ACCENT, tokens::FONT_SANS,
                ),
                onclick: move |_| {
                    if let Some(win) = web_sys::window() {
                        let _ = win.location().reload();
                    }
                },
                "Try again"
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
pub(crate) fn live_view(
    payload: &SessionPayload,
    commands: Coroutine<ClientCommand>,
    cmd_outcome: Signal<CommandOutcome>,
) -> Element {
    match payload {
        SessionPayload::Review(p) => session_view(p, commands, cmd_outcome),
        SessionPayload::Question(p) => question_view(p, commands, cmd_outcome),
        SessionPayload::Direction(p) => direction_view(p, commands, cmd_outcome),
        SessionPayload::Picker(p) => picker_view(p, commands, cmd_outcome),
        // The read-only surfaces: not the desktop's full interactive review, but a
        // basic rendering so the pane isn't blank when the run parks on one.
        SessionPayload::View(p) => view_view(p),
        SessionPayload::VisualReview(p) => visual_review_view(p),
        SessionPayload::Proof(p) => proof_view(p),
    }
}

/// A basic render of a non-blocking artifact **View** the engine mirrored onto
/// the run feed: the run/station header plus the browsable artifacts as a simple
/// list (label + path, a thumbnail when one is present). Read-only here: the
/// full artifact stage lives on the desktop; this just keeps the pane meaningful
/// instead of dropping the payload.
fn view_view(payload: &ViewSessionPayload) -> Element {
    rsx! {
        div { style: "display:flex;flex-direction:column;gap:16px;",
            InteractiveHeader {
                kind: "view".to_string(),
                run: Some(payload.run_slug.clone()),
                title: payload.station.clone(),
                prompt: String::new(),
                context: None,
            }
            if payload.artifacts.is_empty() {
                { empty_note("No artifacts to browse in this view yet.") }
            } else {
                div { style: "display:flex;flex-direction:column;gap:8px;",
                    for art in payload.artifacts.iter() {
                        div {
                            style: format!(
                                "display:flex;align-items:center;gap:12px;padding:10px 14px;\
                                 border:1px solid {};border-radius:8px;background:{};",
                                tokens::BORDER, tokens::SURFACE_RAISED,
                            ),
                            if let Some(thumb) = &art.thumbnail_url {
                                img {
                                    src: "{thumb}",
                                    style: format!(
                                        "width:48px;height:48px;object-fit:cover;border-radius:4px;border:1px solid {};",
                                        tokens::BORDER,
                                    ),
                                }
                            }
                            div { style: "display:flex;flex-direction:column;gap:2px;min-width:0;",
                                span {
                                    style: format!("font-family:{};font-size:14px;color:{};", tokens::FONT_SANS, tokens::TEXT),
                                    "{art.label}"
                                }
                                span {
                                    style: format!(
                                        "font-family:{};font-size:12px;color:{};word-break:break-all;",
                                        tokens::FONT_MONO, tokens::TEXT_MUTED,
                                    ),
                                    "{art.path}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A basic render of a **VisualReview** session: the run/station header, the
/// optional prompt (as markdown), the output screenshot under review, and any
/// annotations already on record. Read-only (dropping pins from the web isn't
/// wired yet), but the payload is no longer discarded to a blank pane.
fn visual_review_view(payload: &VisualReviewSessionPayload) -> Element {
    let prompt = payload.prompt.clone().filter(|p| !p.is_empty());
    let annotations = payload.annotations.clone().filter(|a| !a.is_empty());
    rsx! {
        div { style: "display:flex;flex-direction:column;gap:16px;",
            InteractiveHeader {
                kind: "visual review".to_string(),
                run: payload.run_slug.clone(),
                title: payload.station.clone(),
                prompt: String::new(),
                context: None,
            }
            if let Some(prompt) = prompt {
                { markdown(&prompt, &format!("font-size:15px;color:{};", tokens::TEXT)) }
            }
            if let Some(url) = &payload.screenshot_url {
                img {
                    src: "{url}",
                    style: format!(
                        "width:100%;border-radius:8px;border:1px solid {};",
                        tokens::BORDER,
                    ),
                }
            }
            match annotations {
                Some(a) => rsx! {
                    div { style: "display:flex;flex-direction:column;gap:8px;",
                        for pin in a.pins.iter() {
                            div {
                                style: format!(
                                    "padding:8px 12px;border:1px solid {};border-radius:6px;background:{};\
                                     font-family:{};font-size:13px;color:{};",
                                    tokens::BORDER, tokens::SURFACE_RAISED, tokens::FONT_SANS, tokens::TEXT_MUTED,
                                ),
                                "{pin.note}"
                            }
                        }
                        for comment in a.comments.iter() {
                            div {
                                style: format!(
                                    "padding:8px 12px;border:1px solid {};border-radius:6px;background:{};\
                                     font-family:{};font-size:13px;color:{};",
                                    tokens::BORDER, tokens::SURFACE_RAISED, tokens::FONT_SANS, tokens::TEXT_MUTED,
                                ),
                                "{comment}"
                            }
                        }
                    }
                },
                None => rsx! {
                    { empty_note("Annotate this output from the desktop or the native app to leave feedback.") }
                },
            }
        }
    }
}

/// A basic render of a **Proof** session: the run/station header plus which
/// surface was measured. The full metric panel (web vitals / bench percentiles)
/// lives on the desktop; this states what evidence exists so the payload isn't
/// dropped to a blank pane.
fn proof_view(payload: &ProofSessionPayload) -> Element {
    let surface = format!("{:?}", payload.proof.surface).to_lowercase();
    rsx! {
        div { style: "display:flex;flex-direction:column;gap:16px;",
            InteractiveHeader {
                kind: "proof".to_string(),
                run: payload.run_slug.clone(),
                title: payload.station.clone(),
                prompt: String::new(),
                context: None,
            }
            div {
                style: format!(
                    "padding:12px 14px;border:1px solid {};border-radius:8px;background:{};\
                     font-family:{};font-size:13px;color:{};line-height:1.5;",
                    tokens::BORDER, tokens::SURFACE_RAISED, tokens::FONT_SANS, tokens::TEXT_MUTED,
                ),
                "Objective evidence recorded for the "
                span { style: format!("color:{};", tokens::TEXT), "{surface}" }
                " surface. Open the run on the desktop app for the full metrics."
            }
        }
    }
}

/// A muted, bordered "nothing here" note: the shared empty state for the
/// read-only views, so an empty payload reads as intentional rather than broken.
fn empty_note(text: &str) -> Element {
    rsx! {
        p {
            style: format!(
                "font-family:{};font-size:13px;color:{};margin:0;padding:16px;\
                 border:1px dashed {};border-radius:8px;text-align:center;",
                tokens::FONT_SANS, tokens::TEXT_FAINT, tokens::BORDER,
            ),
            "{text}"
        }
    }
}

/// Render agent-authored prose as formatted markdown through the shared
/// `darkrun_ui` renderer, the same [`darkrun_ui::markdown::to_html`] the desktop
/// uses, so the web surface matches instead of showing raw text. `style` is
/// applied inline to the `.dr-md` container (color/size overrides win over the
/// scoped rules); the `.dr-md` CSS itself is injected once per shell.
fn markdown(text: &str, style: &str) -> Element {
    rsx! {
        div {
            class: "dr-md",
            style: "{style}",
            dangerous_inner_html: darkrun_ui::markdown::to_html(text),
        }
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

            // Gate — the checkpoint controls: Approve clears it, Request changes
            // routes the station back as rework. Both push a Decide command to
            // the host over the tunnel, mirroring the desktop checkpoint.
            if let Some(gate) = payload.gate_type {
                CheckpointGate {
                    session: payload.session_id.clone(),
                    gate_label: format!("A {gate:?} checkpoint is waiting for your decision."),
                    commands,
                    cmd_outcome,
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
                                    { markdown(body, &format!("font-size:13px;color:{};", tokens::TEXT_MUTED)) }
                                }
                            }
                        }
                    },
                    // No outcomes and no briefs yet: say so, so an empty review
                    // reads as "nothing recorded here yet" rather than a blank pane.
                    None => rsx! {
                        { empty_note("No station narrative yet: this review has no recorded briefs or outcomes.") }
                    },
                }
            }
        }
    }
}

/// The checkpoint gate controls for a live review: **Approve** clears the gate,
/// **Request changes** routes the station back as rework with an optional note.
/// Each pushes a [`ClientCommand::Decide`] to the host over the tunnel — the
/// remote mirror of the desktop checkpoint. A `#[component]` so its note signal
/// stays isolated (safe to mount only while a gate is open) instead of adding a
/// hook to the conditionally-rendered review body.
#[component]
fn CheckpointGate(
    session: String,
    gate_label: String,
    commands: Coroutine<ClientCommand>,
    cmd_outcome: Signal<CommandOutcome>,
) -> Element {
    let mut note = use_signal(String::new);

    let gate_box = format!(
        "display:flex;flex-direction:column;gap:12px;\
         padding:14px 16px;border:1px solid {};border-radius:8px;background:{};",
        tokens::ACCENT_STRONG, tokens::SURFACE_RAISED,
    );
    let prompt_style = format!(
        "font-family:{};font-size:14px;color:{};",
        tokens::FONT_SANS, tokens::TEXT,
    );

    // Without a session id the decide endpoint has nothing to target — nudge the
    // operator to the desktop/CLI, as the old approve-only gate did.
    if session.is_empty() {
        return rsx! {
            div { style: "{gate_box}",
                span { style: "{prompt_style}", "{gate_label}" }
                span {
                    style: format!(
                        "font-family:{};font-size:13px;color:{};",
                        tokens::FONT_SANS, tokens::STATUS_WARN,
                    ),
                    "This run's id didn't come through — decide it from the desktop or CLI."
                }
            }
        };
    }

    let approve_btn = format!(
        "padding:8px 18px;border:none;border-radius:6px;cursor:pointer;\
         background:{};color:{};font-family:{};font-size:14px;font-weight:600;",
        tokens::ACCENT, tokens::ON_ACCENT, tokens::FONT_SANS,
    );
    let changes_btn = format!(
        "padding:8px 18px;border:1px solid {};border-radius:6px;cursor:pointer;\
         background:transparent;color:{};font-family:{};font-size:14px;font-weight:600;",
        tokens::STATUS_DANGER, tokens::STATUS_DANGER, tokens::FONT_SANS,
    );
    let note_area = format!(
        "width:100%;box-sizing:border-box;min-height:52px;padding:9px 12px;\
         border-radius:6px;border:1px solid {};background:{};color:{};\
         font-family:{};font-size:13px;resize:vertical;",
        tokens::BORDER, tokens::SURFACE_BASE, tokens::TEXT, tokens::FONT_SANS,
    );

    let approve_session = session.clone();
    rsx! {
        div { style: "display:flex;flex-direction:column;gap:12px;",
            div { style: "{gate_box}",
                span { style: "{prompt_style}", "{gate_label}" }
                textarea {
                    style: "{note_area}",
                    placeholder: "Note for request changes (optional)\u{2026}",
                    oninput: move |evt| note.set(evt.value()),
                }
                div { style: "display:flex;align-items:center;gap:10px;flex-wrap:wrap;",
                    button {
                        style: "{approve_btn}",
                        onclick: move |_| {
                            let mut cmd_outcome = cmd_outcome;
                            cmd_outcome.set(CommandOutcome::Pending);
                            commands.send(ClientCommand::Decide {
                                session: approve_session.clone(),
                                decision: "approved".to_string(),
                                note: None,
                            });
                        },
                        "Approve"
                    }
                    button {
                        style: "{changes_btn}",
                        onclick: move |_| {
                            let mut cmd_outcome = cmd_outcome;
                            cmd_outcome.set(CommandOutcome::Pending);
                            let n = note.read().trim().to_string();
                            commands.send(ClientCommand::Decide {
                                session: session.clone(),
                                decision: "changes_requested".to_string(),
                                note: (!n.is_empty()).then_some(n),
                            });
                        },
                        "Request changes"
                    }
                }
            }
            { command_outcome_note(cmd_outcome) }
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
/// archetype cards. Picking one sends a [`ClientCommand::Direction`] the host
/// routes to `POST /direction/:id/select` — the direction half of the
/// interactive round-trip, no longer misrouted through the question endpoint.
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
                                    commands.send(ClientCommand::Direction {
                                        session: session.clone(),
                                        archetype: id.clone(),
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
/// plus its options. Picking one sends a [`ClientCommand::Picker`] the host
/// routes to `POST /picker/:id/select` — the picker half of the interactive
/// round-trip.
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
                                    commands.send(ClientCommand::Picker {
                                        session: session.clone(),
                                        option: id.clone(),
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
                { markdown(&prompt, &format!("font-size:15px;color:{};", tokens::TEXT)) }
            }
            if let Some(context) = context {
                { markdown(&context, &format!("font-size:13px;color:{};", tokens::TEXT_MUTED)) }
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
                { markdown(&description, &format!("font-size:13px;color:{};", tokens::TEXT_MUTED)) }
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
    fn single_segment_after_in_reads_a_single_segment_id() {
        assert_eq!(single_segment_after_in("/review/abc123", "/review/"), Some("abc123".into()));
        // Trailing slash tolerated.
        assert_eq!(single_segment_after_in("/review/abc123/", "/review/"), Some("abc123".into()));
        // The same helper backs the /runs/<slug> route.
        assert_eq!(single_segment_after_in("/runs/my-run", "/runs/"), Some("my-run".into()));
    }

    #[test]
    fn single_segment_after_in_rejects_non_matching_and_nested_paths() {
        assert_eq!(single_segment_after_in("/", "/review/"), None);
        assert_eq!(single_segment_after_in("/login", "/review/"), None);
        assert_eq!(single_segment_after_in("/review/", "/review/"), None); // empty id
        assert_eq!(single_segment_after_in("/review", "/review/"), None); // no segment
        // A nested path is not a bare single segment.
        assert_eq!(single_segment_after_in("/review/a/b", "/review/"), None);
        // Wrong prefix yields nothing (a /runs path is not a review id).
        assert_eq!(single_segment_after_in("/runs/x", "/review/"), None);
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
