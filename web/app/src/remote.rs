//! The relay client connection — the web app's transport.
//!
//! WASM in a browser can't open raw sockets, so this rides the browser's
//! WebSocket via `gloo-net`, speaking the shared tunnel protocol (the same
//! [`ServerFrame`]/[`ClientFrame`] the native clients use). It connects to the
//! relay's client endpoint, greets with [`ClientFrame::Hello`], and renders each
//! [`ServerFrame::Snapshot`]/`Update` it receives — so the page reads into the
//! live session on connect. A dropped socket reconnects with a fixed backoff
//! (the snapshot-on-connect resync means no state is lost).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use darkrun_api::session::{ReviewSessionPayload, SessionPayload};
use darkrun_api::tunnel::{ClientCommand, ClientFrame, ServerFrame};
use dioxus::prelude::*;
use futures::channel::mpsc::UnboundedReceiver;
use futures::{select, FutureExt, SinkExt, StreamExt};
use gloo_net::websocket::{futures::WebSocket, Message};

/// The live connection state the UI renders. (Not `PartialEq` — the review
/// payload isn't; the UI re-renders on every set, which is what we want for a
/// live feed.)
#[derive(Clone)]
pub enum RemoteState {
    /// No connection target was found in the URL.
    Unconfigured,
    /// Opening the socket / awaiting the first snapshot.
    Connecting,
    /// A live review payload is in hand.
    Live(Box<ReviewSessionPayload>),
    /// The socket dropped; retrying.
    Reconnecting,
}

/// Where to connect: the relay client URL for a session, assembled from the page
/// query (`?relay=wss://…&session=…&token=…`).
#[derive(Clone, PartialEq)]
pub struct Target {
    /// The full `wss://…/relay/client/{session}?token=…` URL.
    pub url: String,
    /// The session id, for display.
    pub session: String,
}

/// Resolve the connection target from the page URL query string, or `None` when
/// the app was opened without one (e.g. the bare landing).
pub fn target_from_url() -> Option<Target> {
    let search = web_sys::window()?.location().search().ok()?;
    let query = search.trim_start_matches('?');
    let mut relay = None;
    let mut session = None;
    let mut token = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("relay", v)) => relay = Some(decode(v)),
            Some(("session", v)) => session = Some(decode(v)),
            Some(("token", v)) => token = Some(decode(v)),
            _ => {}
        }
    }
    let (relay, session, token) = (relay?, session?, token?);
    if relay.is_empty() || session.is_empty() || token.is_empty() {
        return None;
    }
    let url = format!(
        "{}/relay/client/{}?token={}",
        relay.trim_end_matches('/'),
        session,
        token
    );
    Some(Target { url, session })
}

/// Minimal percent-decoding for the query values we read.
fn decode(s: &str) -> String {
    s.replace("%3A", ":").replace("%2F", "/").replace("%2f", "/")
}

/// A monotonic id for outbound commands (the protocol's idempotency/ack key).
fn next_command_id() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    format!("c{}", N.fetch_add(1, Ordering::Relaxed))
}

/// Run the connection loop forever: push live review payloads into `state`, and
/// forward any [`ClientCommand`] arriving on `cmd_rx` to the host as a guarded,
/// acked `Cmd` frame. Reconnects with a fixed backoff after any drop. Commands
/// sent while disconnected are dropped (the operator retries from the live UI).
pub async fn run_connection(
    url: String,
    mut state: Signal<RemoteState>,
    mut cmd_rx: UnboundedReceiver<ClientCommand>,
) {
    loop {
        state.set(RemoteState::Connecting);
        if let Ok(ws) = WebSocket::open(&url) {
            let (mut tx, mut rx) = ws.split();
            // Greet so the host opens this client its session subscription.
            if let Ok(hello) = serde_json::to_string(&ClientFrame::Hello { last_seq: None }) {
                let _ = tx.send(Message::Text(hello)).await;
            }
            // Pump inbound review frames AND outbound commands over the one socket.
            loop {
                select! {
                    msg = rx.next().fuse() => match msg {
                        Some(Ok(Message::Text(t))) => {
                            if let Some(payload) = review_payload(&t) {
                                state.set(RemoteState::Live(Box::new(payload)));
                            }
                        }
                        Some(Ok(_)) => continue,
                        _ => break, // closed/errored
                    },
                    cmd = cmd_rx.next().fuse() => match cmd {
                        Some(command) => {
                            let frame = ClientFrame::Cmd { id: next_command_id(), command };
                            if let Ok(j) = serde_json::to_string(&frame) {
                                if tx.send(Message::Text(j)).await.is_err() {
                                    break;
                                }
                            }
                        }
                        None => {} // the UI's sender was dropped; keep reading
                    },
                }
            }
        }
        state.set(RemoteState::Reconnecting);
        gloo_timers::future::sleep(Duration::from_secs(3)).await;
    }
}

/// Decode a relay text frame to a review payload, if it carries one.
fn review_payload(text: &str) -> Option<ReviewSessionPayload> {
    let frame = serde_json::from_str::<ServerFrame>(text).ok()?;
    let payload = match frame {
        ServerFrame::Snapshot { payload, .. } | ServerFrame::Update { payload, .. } => payload,
        _ => return None,
    };
    match serde_json::from_value::<SessionPayload>(payload).ok()? {
        SessionPayload::Review(p) => Some(p),
        _ => None,
    }
}
