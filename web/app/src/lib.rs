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
mod remote;

use darkrun_api::session::ReviewSessionPayload;
use darkrun_ui::prelude::*;
use darkrun_ui::tokens;

use banner::InstallBanner;
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
        return rsx! { LoginPage {} };
    }

    let state = use_signal(|| RemoteState::Unconfigured);

    // Open the relay connection on mount when the URL names a target.
    use_effect(move || {
        if let Some(target) = target_from_url() {
            spawn(run_connection(target.url, state));
        }
    });

    let shell = format!(
        "min-height:100vh;background:{};color:{};font-family:{};",
        tokens::SURFACE_BASE,
        tokens::TEXT,
        tokens::FONT_SANS,
    );

    rsx! {
        document::Title { "darkrun" }
        div { style: "{shell}",
            Header {}
            InstallBanner {}
            main { style: "max-width:880px;margin:0 auto;padding:24px 20px 64px;",
                match state() {
                    RemoteState::Unconfigured => rsx! { NoTarget {} },
                    RemoteState::Connecting => rsx! { Status { text: "Connecting to your run\u{2026}" } },
                    RemoteState::Reconnecting => rsx! { Status { text: "Reconnecting\u{2026}" } },
                    RemoteState::Live(payload) => session_view(&payload),
                }
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

/// The live review surface for one run: identity, the phase strip, and the
/// stations with their status. A plain function (not a `#[component]`) because
/// the payload isn't `PartialEq` — it's re-rendered on every live update.
fn session_view(payload: &ReviewSessionPayload) -> Element {
    let run = payload.run_slug.clone().unwrap_or_else(|| "run".to_string());
    let phase = active_phase(&payload);

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

            // Gate banner — what needs the operator now.
            if let Some(gate) = payload.gate_type {
                div {
                    style: format!(
                        "padding:12px 16px;border:1px solid {};border-radius:8px;background:{};\
                         font-family:{};font-size:14px;color:{};",
                        tokens::ACCENT_STRONG, tokens::SURFACE_RAISED, tokens::FONT_SANS, tokens::TEXT,
                    ),
                    { format!("A {gate:?} checkpoint is waiting for your decision.") }
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
        }
    }
}

/// The active phase as a `darkrun-ui` [`Phase`], from the payload's current state.
fn active_phase(payload: &ReviewSessionPayload) -> Option<Phase> {
    let rp = payload.current_state.as_ref()?.phase.as_ref()?;
    let name = serde_json::to_value(rp).ok()?;
    Phase::from_name(name.as_str()?)
}
