//! The `/runs/:slug` surface — reaching a LIVE run from the clean link.
//!
//! The engine mints `https://app.darkrun.ai/runs/<slug>` (see `darkrun_mcp`'s
//! `run_web_url`): a CLEAN link with NO secret in it. This module turns that link
//! into a live connection:
//!
//! 1. restore the persisted Firebase ID token ([`firebase::restore_session`]) —
//!    absent → bounce to `/login` with a `return_to` back here;
//! 2. resolve the relay-attach descriptor from the AUTHENTICATED
//!    `GET /api/runs/<slug>/relay` (relay base + session + client token) — the
//!    relay/session/token never touch the URL or history;
//! 3. assemble the relay client URL the tunnel expects
//!    (`{relay}/relay/client/{session}?token={token}`, the shape
//!    [`remote::target_from_url`] builds) and drive the existing live review
//!    surface ([`remote::run_connection`] + [`crate::live_view`]).
//!
//! A `404` (no live host for this owner) renders a clear "not live" state with a
//! path back to the workspace; a `401` (token expired) re-logs in.

use darkrun_api::tunnel::ClientCommand;
use darkrun_ui::prelude::*;
use darkrun_ui::tokens;
use futures::channel::mpsc::UnboundedReceiver;
use gloo_net::http::Request;
use serde::Deserialize;

use crate::banner::InstallBanner;
use crate::firebase;
use crate::remote::{run_connection, CommandOutcome, RemoteState};
use crate::{live_view, Header, Status};

/// The relay-attach descriptor `GET /api/runs/{slug}/relay` returns (mirrors the
/// server's `darkrun_web::RelayDescriptor`).
#[derive(Clone, PartialEq, Deserialize)]
pub struct RelayDescriptor {
    /// The public relay base URL (e.g. `wss://relay.darkrun.ai`).
    pub relay_url: String,
    /// The relay session id — the run slug.
    pub session: String,
    /// The bearer token to authenticate the relay attach (the caller's own token).
    pub client_token: String,
}

/// Why the descriptor fetch didn't yield a connectable run.
enum DescriptorError {
    /// `404` — no live host for this run (or not the caller's run). Not reachable.
    NotLive,
    /// `401` — the token is invalid/expired; re-login.
    Unauthorized,
    /// A transport / decode / other server error.
    Other(String),
}

/// Where the resolve-then-connect flow currently sits (the pre-connection phase
/// the live `RemoteState` doesn't cover).
#[derive(Clone, PartialEq)]
enum Phase {
    /// Restoring the session / fetching the descriptor.
    Resolving,
    /// The descriptor resolved; the live connection drives `state` from here.
    Connected,
    /// `404` — the run isn't live (or isn't the caller's).
    NotLive,
    /// Bouncing to `/login` (no session, or token expired).
    Redirecting,
    /// The resolve failed for another reason.
    Error(String),
}

/// The `/runs/:slug` component: resolve the relay descriptor for `slug`, then
/// drive the live review surface (or a clear non-live/error state).
#[component]
pub fn RunView(slug: String) -> Element {
    let phase = use_signal(|| Phase::Resolving);
    let state = use_signal(|| RemoteState::Unconfigured);
    // The outcome of the operator's most recent command (approve/answer), so a
    // remote action reflects the host's ack rather than being a silent no-op.
    let cmd_outcome = use_signal(|| CommandOutcome::Idle);

    // Resolve + connect in ONE coroutine so its handle doubles as the command
    // channel the live UI sends on (approve a gate, answer a question).
    let commands = use_coroutine({
        let slug = slug.clone();
        move |cmd_rx: UnboundedReceiver<ClientCommand>| {
            // The coroutine init is `FnMut`, so clone the slug into the (once-run)
            // future rather than moving the captured copy out of the closure.
            let slug = slug.clone();
            let mut phase = phase;
            async move {
                // 1. The persisted Firebase token is the bearer for the descriptor
                //    API. Absent → bounce to /login, returning here after sign-in.
                let Some(token) = firebase::restore_session().await else {
                    phase.set(Phase::Redirecting);
                    redirect_to_login(&slug);
                    return;
                };
                // 2. Resolve the relay/session/token from the authenticated API.
                match fetch_relay_descriptor(&firebase::web_base(), &slug, &token).await {
                    Ok(desc) => {
                        // 3. Assemble the client URL the tunnel expects and drive
                        //    the live surface.
                        let url = client_url(&desc.relay_url, &desc.session, &desc.client_token);
                        phase.set(Phase::Connected);
                        run_connection(url, state, cmd_rx, cmd_outcome).await;
                    }
                    Err(DescriptorError::NotLive) => phase.set(Phase::NotLive),
                    Err(DescriptorError::Unauthorized) => {
                        // The token expired/was rejected — re-login (a fresh sign-in
                        // re-mints it), returning here.
                        phase.set(Phase::Redirecting);
                        redirect_to_login(&slug);
                    }
                    Err(DescriptorError::Other(e)) => phase.set(Phase::Error(e)),
                }
            }
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
                match phase() {
                    Phase::Resolving => rsx! { Status { text: "Resolving your run\u{2026}" } },
                    Phase::Redirecting => rsx! { Status { text: "Taking you to sign in\u{2026}" } },
                    Phase::NotLive => rsx! { NotLive { slug: slug.clone() } },
                    Phase::Error(e) => rsx! { ResolveError { message: e } },
                    // Connected: the live connection now drives `state`.
                    Phase::Connected => match state() {
                        RemoteState::Live(payload) => live_view(&payload, commands, cmd_outcome),
                        RemoteState::Reconnecting => rsx! { Status { text: "Reconnecting\u{2026}" } },
                        // Unconfigured (briefly, before the socket opens) or
                        // Connecting both read as connecting.
                        _ => rsx! { Status { text: "Connecting to your run\u{2026}" } },
                    },
                }
            }
        }
    }
}

/// The "this run isn't live" surface: a `404` from the descriptor API means no
/// live host is reachable for this account (the run may be finished, or running
/// on a machine that isn't signed in). Point back at the workspace.
#[component]
fn NotLive(slug: String) -> Element {
    rsx! {
        div { style: "text-align:center;padding:48px 0;",
            p {
                style: format!(
                    "font-family:{};font-size:12px;color:{};letter-spacing:.06em;\
                     text-transform:uppercase;margin:0 0 6px;",
                    tokens::FONT_MONO, tokens::TEXT_FAINT,
                ),
                "run"
            }
            p {
                style: format!(
                    "font-family:{};font-size:18px;color:{};margin:0 0 12px;word-break:break-all;",
                    tokens::FONT_SANS, tokens::TEXT,
                ),
                "{slug}"
            }
            p {
                style: format!(
                    "font-family:{};font-size:13px;color:{};margin:0 auto 16px;max-width:520px;line-height:1.5;",
                    tokens::FONT_SANS, tokens::TEXT_MUTED,
                ),
                "This run isn't live right now \u{2014} it may have finished, or it's running \
                 on a machine that isn't signed in for remote access. Start it with \
                 /darkrun:darkrun-login on that machine, then reopen this link."
            }
            a {
                href: "/",
                style: format!(
                    "display:inline-block;padding:8px 18px;border-radius:6px;text-decoration:none;\
                     background:{};color:{};font-family:{};font-size:14px;font-weight:600;",
                    tokens::ACCENT, tokens::ON_ACCENT, tokens::FONT_SANS,
                ),
                "Back to your workspace"
            }
        }
    }
}

/// A resolve failure that isn't a clean 404/401 (transport / decode / 5xx).
#[component]
fn ResolveError(message: String) -> Element {
    rsx! {
        div { style: "text-align:center;padding:48px 0;",
            p {
                style: format!(
                    "font-family:{};font-size:16px;color:{};margin:0 0 8px;",
                    tokens::FONT_SANS, tokens::TEXT,
                ),
                "Couldn't reach this run."
            }
            p {
                style: format!(
                    "font-family:{};font-size:13px;color:{};margin:0;",
                    tokens::FONT_SANS, tokens::TEXT_MUTED,
                ),
                "{message}"
            }
        }
    }
}

/// Fetch the relay-attach descriptor from the authenticated
/// `GET /api/runs/{slug}/relay`, mapping the status to a [`DescriptorError`].
async fn fetch_relay_descriptor(
    web_base: &str,
    slug: &str,
    token: &str,
) -> Result<RelayDescriptor, DescriptorError> {
    let url = descriptor_url(web_base, slug);
    let resp = Request::get(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| DescriptorError::Other(e.to_string()))?;
    match resp.status() {
        200 => resp
            .json::<RelayDescriptor>()
            .await
            .map_err(|e| DescriptorError::Other(e.to_string())),
        401 => Err(DescriptorError::Unauthorized),
        404 => Err(DescriptorError::NotLive),
        s => Err(DescriptorError::Other(format!("the server returned {s}"))),
    }
}

/// The descriptor endpoint URL for `slug` on `web_base`
/// (`{web_base}/api/runs/{slug}/relay`). Pure, so it's unit-tested.
fn descriptor_url(web_base: &str, slug: &str) -> String {
    format!("{}/api/runs/{}/relay", web_base.trim_end_matches('/'), slug)
}

/// Assemble the relay client URL the tunnel expects from the descriptor's parts:
/// `{relay_url}/relay/client/{session}?token={token}` — the exact shape
/// [`remote::target_from_url`](crate::remote) builds from the query. Pure, so
/// it's unit-tested.
fn client_url(relay_url: &str, session: &str, token: &str) -> String {
    format!(
        "{}/relay/client/{}?token={}",
        relay_url.trim_end_matches('/'),
        session,
        token
    )
}

/// Redirect to `/login` with a `return_to` back to this run, so a fresh sign-in
/// lands the user back on the live run (not the bare workspace).
fn redirect_to_login(slug: &str) {
    if let Some(win) = web_sys::window() {
        let return_to = encode_component(&format!("/runs/{slug}"));
        let _ = win.location().set_href(&format!("/login?return_to={return_to}"));
    }
}

/// Percent-encode a query-parameter value (so a path's `/` survives as `%2F`,
/// which the login page decodes back via `firebase::query_param`).
fn encode_component(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_url_is_the_authenticated_relay_endpoint() {
        assert_eq!(
            descriptor_url("https://darkrun.ai", "quiet-canyon"),
            "https://darkrun.ai/api/runs/quiet-canyon/relay"
        );
        // A trailing slash on the base is trimmed (no double slash).
        assert_eq!(
            descriptor_url("https://darkrun.ai/", "quiet-canyon"),
            "https://darkrun.ai/api/runs/quiet-canyon/relay"
        );
    }

    #[test]
    fn client_url_matches_the_tunnel_attach_shape() {
        // Exactly the shape remote::target_from_url assembles from ?relay&session&token.
        assert_eq!(
            client_url("wss://relay.darkrun.ai", "sess-1", "tok"),
            "wss://relay.darkrun.ai/relay/client/sess-1?token=tok"
        );
        // A trailing slash on the relay base is trimmed.
        assert_eq!(
            client_url("wss://relay.darkrun.ai/", "sess-1", "tok"),
            "wss://relay.darkrun.ai/relay/client/sess-1?token=tok"
        );
    }

    #[test]
    fn encode_component_percent_encodes_path_separators() {
        // `/runs/slug` → the login page decodes %2F back to `/`.
        assert_eq!(encode_component("/runs/quiet-canyon"), "%2Fruns%2Fquiet-canyon");
        // Unreserved chars pass through.
        assert_eq!(encode_component("abc-123_.~"), "abc-123_.~");
    }
}
