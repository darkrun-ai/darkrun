//! The remote tunnel **relay** — a stateless reverse-WebSocket bridge.
//!
//! Remote access to a run does NOT sync its state (see `firestore/SCHEMA.md`).
//! The host (the machine running the agent's live MCP session) serves the run
//! over its in-process HTTP/WS — the same surface the desktop reads on loopback.
//! A remote client can't reach that directly (the host is behind NAT), so:
//!
//! - the **host dials OUTBOUND** to `GET /relay/host/{session}` and parks an open
//!   WebSocket here (outbound always traverses NAT — no inbound port);
//! - a **client** connects to `GET /relay/client/{session}`;
//! - the relay **bridges frames** between them — host→clients fans out to every
//!   attached client (the seam for multi-party "channels"), client→host forwards
//!   to the single host socket.
//!
//! The relay holds NO run state and never inspects frame contents — it shuttles
//! opaque WebSocket frames, so the host's existing review protocol rides over it
//! unchanged. Authorization binds at registration: the host's verified token
//! fixes the session's owner account, and a client may attach only if its token
//! resolves to that same owner (later: the same channel). Why a relay and not
//! WebRTC: see the `tunnel-transport` decision.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;

/// One opaque WebSocket frame shuttled across the bridge. The relay treats both
/// kinds as payload — it never parses them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// A UTF-8 text frame (the review protocol's JSON messages).
    Text(String),
    /// A binary frame.
    Binary(Vec<u8>),
}

/// The live state of one tunnelled session: the host's outbound sink plus every
/// attached client's sink. Senders are unbounded mpsc halves drained by each
/// socket's writer task.
struct SessionConn {
    /// The account that owns this session (set by the host at registration);
    /// a client must resolve to the same owner to attach.
    owner: String,
    /// Push here to send a frame TO the host.
    host_tx: mpsc::UnboundedSender<Frame>,
    /// Push to a client's entry to send a frame TO that client.
    clients: HashMap<u64, mpsc::UnboundedSender<Frame>>,
}

/// Why an attach was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachError {
    /// No host is registered for that session (nothing live to reach).
    NoHost,
    /// The client's owner doesn't match the session's owner.
    Forbidden,
}

/// The in-memory bridge: `session_id -> SessionConn`. Stateless across restarts
/// (a dropped relay just forces hosts + clients to reconnect). Cloneable handle
/// semantics are provided by wrapping in an `Arc` at the router layer.
#[derive(Default)]
pub struct Relay {
    sessions: Mutex<HashMap<String, SessionConn>>,
    next_client_id: AtomicU64,
}

impl Relay {
    /// A fresh, empty relay.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `session` as hosted by `owner`, returning the receiver the host's
    /// writer task drains (frames bound for the host). Refuses (`None`) if a host
    /// is already registered for that session — one host per session.
    pub fn register_host(
        &self,
        session: &str,
        owner: &str,
    ) -> Option<mpsc::UnboundedReceiver<Frame>> {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.contains_key(session) {
            return None;
        }
        let (host_tx, host_rx) = mpsc::unbounded_channel();
        sessions.insert(
            session.to_string(),
            SessionConn {
                owner: owner.to_string(),
                host_tx,
                clients: HashMap::new(),
            },
        );
        Some(host_rx)
    }

    /// Tear down a session when its host disconnects: drop the session entry so
    /// every attached client's sender closes (ending their writer tasks) and a
    /// later attach sees `NoHost`.
    pub fn drop_host(&self, session: &str) {
        self.sessions.lock().unwrap().remove(session);
    }

    /// Attach a client to `session` (owned by `owner`), returning `(client_id,
    /// receiver)` — the receiver feeds the client's writer task. Refuses if no
    /// host is live (`NoHost`) or the owner doesn't match (`Forbidden`).
    pub fn attach_client(
        &self,
        session: &str,
        owner: &str,
    ) -> Result<(u64, mpsc::UnboundedReceiver<Frame>), AttachError> {
        let mut sessions = self.sessions.lock().unwrap();
        let conn = sessions.get_mut(session).ok_or(AttachError::NoHost)?;
        if conn.owner != owner {
            return Err(AttachError::Forbidden);
        }
        let id = self.next_client_id.fetch_add(1, Ordering::Relaxed);
        let (client_tx, client_rx) = mpsc::unbounded_channel();
        conn.clients.insert(id, client_tx);
        Ok((id, client_rx))
    }

    /// Detach a client (its socket closed). Idempotent.
    pub fn detach_client(&self, session: &str, client_id: u64) {
        if let Some(conn) = self.sessions.lock().unwrap().get_mut(session) {
            conn.clients.remove(&client_id);
        }
    }

    /// Route a frame the HOST sent: fan it out to every attached client. Drops
    /// senders that have closed. No-op if the session is gone.
    pub fn from_host(&self, session: &str, frame: Frame) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(conn) = sessions.get_mut(session) {
            conn.clients.retain(|_, tx| tx.send(frame.clone()).is_ok());
        }
    }

    /// Route a frame a CLIENT sent: forward it to the single host. Returns false
    /// if there's no live host (the client should disconnect).
    pub fn from_client(&self, session: &str, frame: Frame) -> bool {
        let sessions = self.sessions.lock().unwrap();
        match sessions.get(session) {
            Some(conn) => conn.host_tx.send(frame).is_ok(),
            None => false,
        }
    }

    /// How many clients are attached to `session` (0 if absent). Observability.
    pub fn client_count(&self, session: &str) -> usize {
        self.sessions
            .lock()
            .unwrap()
            .get(session)
            .map(|c| c.clients.len())
            .unwrap_or(0)
    }

    /// Whether a host is registered for `session`.
    pub fn has_host(&self, session: &str) -> bool {
        self.sessions.lock().unwrap().contains_key(session)
    }
}

/// Resolves a bearer token to the account that owns it — the relay's authz seam.
/// Production verifies a **Firebase ID token** (its signature + audience) and
/// returns the `uid`; that impl wires in with the auth slice. Kept behind a trait
/// so the bridge is testable offline and the token source can evolve.
pub trait RelayAuth: Send + Sync {
    /// The account id a token resolves to, or `None` if it's invalid/expired.
    fn account_for(&self, token: &str) -> Option<String>;
}

/// A development verifier that treats the token AS the account id. Only for local
/// runs + tests — never wire this in production (it trusts any caller). Enable
/// deliberately; the Firebase verifier replaces it.
pub struct DevTokenAuth;

impl RelayAuth for DevTokenAuth {
    fn account_for(&self, token: &str) -> Option<String> {
        let t = token.trim();
        (!t.is_empty()).then(|| t.to_string())
    }
}

/// The relay router's shared state: the bridge hub + the token verifier.
#[derive(Clone)]
pub struct RelayState {
    relay: Arc<Relay>,
    auth: Arc<dyn RelayAuth>,
}

impl RelayState {
    /// Build relay state from a hub + a token verifier.
    pub fn new(relay: Arc<Relay>, auth: Arc<dyn RelayAuth>) -> Self {
        Self { relay, auth }
    }
}

/// The bearer token, carried as a query param on the WS upgrade (browsers can't
/// set headers on a WebSocket handshake, so the token rides the URL — over TLS).
#[derive(Deserialize)]
struct RelayQuery {
    token: String,
}

/// Mount the relay endpoints: the host's outbound park and the client attach.
pub fn relay_router(state: RelayState) -> Router {
    Router::new()
        .route("/relay/host/{session}", get(host_ws))
        .route("/relay/client/{session}", get(client_ws))
        .with_state(state)
}

/// Convert a relay [`Frame`] into an axum WS message.
fn into_msg(frame: Frame) -> Message {
    match frame {
        Frame::Text(t) => Message::Text(t.into()),
        Frame::Binary(b) => Message::Binary(b.into()),
    }
}

/// Convert an incoming WS message into a relay [`Frame`], or `None` for control
/// frames (ping/pong/close) the relay doesn't shuttle.
fn from_msg(msg: Message) -> Option<Frame> {
    match msg {
        Message::Text(t) => Some(Frame::Text(t.to_string())),
        Message::Binary(b) => Some(Frame::Binary(b.to_vec())),
        _ => None,
    }
}

/// `GET /relay/host/{session}` — the host parks its outbound socket here.
#[cfg(not(tarpaulin_include))] // WS upgrade + socket loop — exercised via the Relay hub tests
async fn host_ws(
    Path(session): Path<String>,
    Query(q): Query<RelayQuery>,
    State(state): State<RelayState>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(owner) = state.auth.account_for(&q.token) else {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    };
    ws.on_upgrade(move |socket| run_host(state.relay, session, owner, socket))
}

/// `GET /relay/client/{session}` — a client attaches to a live session.
#[cfg(not(tarpaulin_include))] // WS upgrade + socket loop — exercised via the Relay hub tests
async fn client_ws(
    Path(session): Path<String>,
    Query(q): Query<RelayQuery>,
    State(state): State<RelayState>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(owner) = state.auth.account_for(&q.token) else {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    };
    ws.on_upgrade(move |socket| run_client(state.relay, session, owner, socket))
}

/// Drive a host socket: register it, fan its frames out to clients, and feed it
/// frames clients send. Tears the session down when the socket closes.
#[cfg(not(tarpaulin_include))] // socket I/O loop
async fn run_host(relay: Arc<Relay>, session: String, owner: String, socket: WebSocket) {
    let Some(mut host_rx) = relay.register_host(&session, &owner) else {
        return; // a host already holds this session
    };
    let (mut sink, mut stream) = socket.split();
    let mut writer = tokio::spawn(async move {
        while let Some(frame) = host_rx.recv().await {
            if sink.send(into_msg(frame)).await.is_err() {
                break;
            }
        }
    });
    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(msg)) => {
                    if let Some(frame) = from_msg(msg) {
                        relay.from_host(&session, frame);
                    }
                }
                _ => break, // socket closed/errored
            },
            _ = &mut writer => break, // writer ended (rx closed)
        }
    }
    relay.drop_host(&session);
    writer.abort();
}

/// Drive a client socket: attach it, forward its frames to the host, and write
/// host frames back. Detaches on close.
#[cfg(not(tarpaulin_include))] // socket I/O loop
async fn run_client(relay: Arc<Relay>, session: String, owner: String, socket: WebSocket) {
    let (id, mut client_rx) = match relay.attach_client(&session, &owner) {
        Ok(pair) => pair,
        Err(_) => return, // no host / forbidden — drop the upgrade
    };
    let (mut sink, mut stream) = socket.split();
    let mut writer = tokio::spawn(async move {
        while let Some(frame) = client_rx.recv().await {
            if sink.send(into_msg(frame)).await.is_err() {
                break;
            }
        }
    });
    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(msg)) => {
                    if let Some(frame) = from_msg(msg) {
                        // Host gone → stop pumping this client.
                        if !relay.from_client(&session, frame) {
                            break;
                        }
                    }
                }
                _ => break,
            },
            _ = &mut writer => break,
        }
    }
    relay.detach_client(&session, id);
    writer.abort();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_token_auth_resolves_token_to_account() {
        assert_eq!(DevTokenAuth.account_for("acct-a"), Some("acct-a".to_string()));
        assert_eq!(DevTokenAuth.account_for("  acct-b "), Some("acct-b".to_string()));
        assert_eq!(DevTokenAuth.account_for(""), None);
    }

    #[test]
    fn host_registers_once_and_clients_attach_to_their_owner() {
        let relay = Relay::new();
        assert!(!relay.has_host("s1"));

        let _host_rx = relay.register_host("s1", "acct-a").expect("first host registers");
        assert!(relay.has_host("s1"));
        // One host per session.
        assert!(relay.register_host("s1", "acct-a").is_none());

        // Owner match attaches; mismatch is forbidden.
        let (cid, _crx) = relay.attach_client("s1", "acct-a").expect("owner attaches");
        assert_eq!(relay.client_count("s1"), 1);
        assert!(matches!(
            relay.attach_client("s1", "acct-b"),
            Err(AttachError::Forbidden)
        ));

        // No host → NoHost.
        assert!(matches!(
            relay.attach_client("ghost", "acct-a"),
            Err(AttachError::NoHost)
        ));

        relay.detach_client("s1", cid);
        assert_eq!(relay.client_count("s1"), 0);
    }

    #[test]
    fn host_frames_fan_out_to_all_clients() {
        let relay = Relay::new();
        let _host_rx = relay.register_host("s", "acct").unwrap();
        let (_c1, mut rx1) = relay.attach_client("s", "acct").unwrap();
        let (_c2, mut rx2) = relay.attach_client("s", "acct").unwrap();

        relay.from_host("s", Frame::Text("hello".into()));

        assert_eq!(rx1.try_recv().unwrap(), Frame::Text("hello".into()));
        assert_eq!(rx2.try_recv().unwrap(), Frame::Text("hello".into()));
    }

    #[test]
    fn client_frames_forward_to_the_host() {
        let relay = Relay::new();
        let mut host_rx = relay.register_host("s", "acct").unwrap();
        let (_c, _crx) = relay.attach_client("s", "acct").unwrap();

        assert!(relay.from_client("s", Frame::Text("answer".into())));
        assert_eq!(host_rx.try_recv().unwrap(), Frame::Text("answer".into()));

        // After the host drops, a client frame has nowhere to go.
        relay.drop_host("s");
        assert!(!relay.from_client("s", Frame::Text("late".into())));
    }

    #[test]
    fn dropping_the_host_closes_client_receivers() {
        let relay = Relay::new();
        let _host_rx = relay.register_host("s", "acct").unwrap();
        let (_c, mut crx) = relay.attach_client("s", "acct").unwrap();

        relay.drop_host("s");
        // The session (and thus the client's sender) is gone → recv ends.
        assert!(crx.try_recv().is_err());
        assert!(!relay.has_host("s"));
    }

    // End-to-end over real sockets: prove the WS handlers + bridge, not just the
    // hub. Host parks, client attaches, a frame crosses each way.
    #[tokio::test]
    async fn bridges_frames_end_to_end_over_websockets() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as T;

        let relay = Arc::new(Relay::new());
        let state = RelayState::new(relay.clone(), Arc::new(DevTokenAuth));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, relay_router(state)).await.unwrap();
        });

        // Host parks its outbound socket.
        let (mut host, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}/relay/host/s1?token=acct"))
                .await
                .expect("host connects");
        // Wait for the host's on_upgrade to actually register before attaching.
        for _ in 0..100 {
            if relay.has_host("s1") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(relay.has_host("s1"));

        let (mut client, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}/relay/client/s1?token=acct"))
                .await
                .expect("client attaches");

        // host → client
        host.send(T::Text("from-host".into())).await.unwrap();
        let got = client.next().await.unwrap().unwrap();
        assert_eq!(got.into_text().unwrap().as_str(), "from-host");

        // client → host
        client.send(T::Text("from-client".into())).await.unwrap();
        let got = host.next().await.unwrap().unwrap();
        assert_eq!(got.into_text().unwrap().as_str(), "from-client");
    }

    #[tokio::test]
    async fn client_with_wrong_owner_is_refused() {
        let relay = Arc::new(Relay::new());
        let state = RelayState::new(relay.clone(), Arc::new(DevTokenAuth));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, relay_router(state)).await.unwrap();
        });

        let (_host, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}/relay/host/s1?token=owner-a"))
                .await
                .unwrap();
        for _ in 0..100 {
            if relay.has_host("s1") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        // A different owner upgrades, but run_client refuses the attach and closes
        // the socket immediately — it never joins the session.
        let (_intruder, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}/relay/client/s1?token=owner-b"))
                .await
                .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(relay.client_count("s1"), 0, "wrong-owner client must not attach");
    }
}
