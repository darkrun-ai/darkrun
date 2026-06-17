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

use darkrun_api::session::ReviewSessionPayload;
use darkrun_ui::prelude::*;
use darkrun_ui::tokens;

use banner::InstallBanner;
use darkrun_api::tunnel::ClientCommand;
use futures::channel::mpsc::UnboundedReceiver;

use login::LoginPage;
use remote::{run_connection, target_from_url, RemoteState};

/// Whether the current page path is the login route.
fn is_login_path() -> bool {
    web_sys::window()
        .and_then(|w| w.location().pathname().ok())
        .map(|p| p.trim_end_matches('/').ends_with("/login"))
        .unwrap_or(false)
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

    let state = use_signal(|| RemoteState::Unconfigured);

    // The connection runs as a coroutine so its handle doubles as the command
    // channel: the UI sends a `ClientCommand` (approve a gate, file feedback) and
    // the connection forwards it to the host over the tunnel.
    let commands = use_coroutine(move |cmd_rx: UnboundedReceiver<ClientCommand>| async move {
        if let Some(target) = target_from_url() {
            run_connection(target.url, state, cmd_rx).await;
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
                    RemoteState::Unconfigured => rsx! { NoTarget {} },
                    RemoteState::Connecting => rsx! { Status { text: "Connecting to your run\u{2026}" } },
                    RemoteState::Reconnecting => rsx! { Status { text: "Reconnecting\u{2026}" } },
                    RemoteState::Live(payload) => session_view(&payload, commands),
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
            Wordmark { variant: WordmarkVariant::Outlined }
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

/// The live review surface for one run: identity, the phase strip, the stations,
/// the station narrative, and — when a gate is open — an Approve action that
/// pushes a command back to the host over the tunnel. A plain function (not a
/// `#[component]`) because the payload isn't `PartialEq` — it's re-rendered on
/// every live update. `commands` forwards operator actions to the connection.
fn session_view(payload: &ReviewSessionPayload, commands: Coroutine<ClientCommand>) -> Element {
    let run = payload.run_slug.clone().unwrap_or_else(|| "run".to_string());
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
                    "{run}"
                }
                StationPipeline { dots: strip_for(phase), labels: true }
            }

            // Gate — what needs the operator now, with the Approve action that
            // advances the run past it (pushed to the host over the tunnel).
            if let Some(gate) = payload.gate_type {
                {
                    let run = run.clone();
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
                            button {
                                style: format!(
                                    "padding:8px 18px;border:none;border-radius:6px;cursor:pointer;\
                                     background:{};color:{};font-family:{};font-size:14px;font-weight:600;",
                                    tokens::ACCENT, tokens::ON_ACCENT, tokens::FONT_SANS,
                                ),
                                onclick: move |_| commands.send(ClientCommand::Advance { run: run.clone() }),
                                "Approve"
                            }
                        }
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

/// The active phase as a `darkrun-ui` [`Phase`], from the payload's current state.
fn active_phase(payload: &ReviewSessionPayload) -> Option<Phase> {
    let rp = payload.current_state.as_ref()?.phase.as_ref()?;
    let name = serde_json::to_value(rp).ok()?;
    Phase::from_name(name.as_str()?)
}
