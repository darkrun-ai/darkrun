//! The remote tunnel **relay** — a client-addressed reverse-WebSocket bridge.
//!
//! Remote access to a run does NOT sync its state (see `firestore/SCHEMA.md`).
//! The host (the machine running the agent's live MCP session) serves the run
//! over its in-process HTTP/WS — the same surface the desktop reads on loopback.
//! A remote client can't reach that directly (the host is behind NAT), so:
//!
//! - the **host dials OUTBOUND** to `GET /relay/host/{session}` and parks an open
//!   WebSocket here (outbound always traverses NAT — no inbound port);
//! - a **client** connects to `GET /relay/client/{session}`;
//! - the relay **routes per client**: each client is addressed by an id, so the
//!   host can open that client its OWN local session subscription (which fires
//!   the snapshot the local server pushes on connect) and reply to just that
//!   client. Without per-client routing a late joiner would miss the snapshot and
//!   see nothing until the next update.
//!
//! Wire protocol:
//! - **client ↔ relay**: RAW review frames — the client speaks the host's review
//!   protocol verbatim; the relay never parses the payload.
//! - **relay → host** ([`HostEvent`]): `join` / `leave` / `msg{client,data}` —
//!   so the host learns of each client and its frames.
//! - **host → relay** ([`HostCmd`]): `to{client,data}` — route a frame to one
//!   client. The relay parses only this thin routing envelope, never the payload.
//!
//! Authorization binds at registration: the host's verified token fixes the
//! session's owner account, and a client may attach only if its token resolves to
//! that same owner (later: the same channel). Why a relay and not WebRTC: see the
//! `tunnel-transport` decision.

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
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// One opaque review frame shuttled across the bridge. The relay never parses the
/// payload — both kinds are carried verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// A UTF-8 text frame (the review protocol's JSON messages).
    Text(String),
    /// A binary frame.
    Binary(Vec<u8>),
}

/// What the relay delivers TO the host over its socket: a client lifecycle event
/// or a client frame, each tagged with the client id so the host can open/close
/// per-client local subscriptions and attribute incoming commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HostEvent {
    /// A client attached — the host should open its session subscription and
    /// stream it back addressed to this client.
    Join {
        /// The relay-assigned client id.
        client: u64,
    },
    /// A client detached — the host should tear down that client's subscription.
    Leave {
        /// The client id that left.
        client: u64,
    },
    /// A frame the client sent (a command — answer / advance / feedback).
    Msg {
        /// The originating client id.
        client: u64,
        /// The client's raw text frame.
        data: String,
    },
}

/// What the host sends back to the relay: route a frame to a specific client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HostCmd {
    /// Deliver `data` to client `client` (e.g. a snapshot or a live update from
    /// that client's session subscription).
    To {
        /// The destination client id.
        client: u64,
        /// The raw text frame to deliver.
        data: String,
    },
}

/// The live state of one tunnelled session: the host's event sink plus every
/// attached client's frame sink.
struct SessionConn {
    /// The account that owns this session; a client must resolve to the same
    /// owner to attach.
    owner: String,
    /// Push here to deliver a [`HostEvent`] to the host.
    host_tx: mpsc::UnboundedSender<HostEvent>,
    /// Per-client frame sinks, keyed by client id.
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
/// (a dropped relay just forces hosts + clients to reconnect).
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
    /// writer task drains ([`HostEvent`]s bound for the host). Refuses (`None`) if
    /// a host is already registered — one host per session.
    pub fn register_host(
        &self,
        session: &str,
        owner: &str,
    ) -> Option<mpsc::UnboundedReceiver<HostEvent>> {
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

    /// Tear down a session when its host disconnects: drop the entry so every
    /// attached client's sender closes (ending their writer tasks) and a later
    /// attach sees `NoHost`.
    pub fn drop_host(&self, session: &str) {
        self.sessions.lock().unwrap().remove(session);
    }

    /// Attach a client to `session` (owned by `owner`), returning `(client_id,
    /// receiver)` — the receiver feeds the client's writer task. Also signals the
    /// host with [`HostEvent::Join`] so it opens this client's subscription.
    /// Refuses if no host is live (`NoHost`) or the owner mismatches (`Forbidden`).
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
        // Tell the host a client joined (best-effort; host may be mid-teardown).
        let _ = conn.host_tx.send(HostEvent::Join { client: id });
        Ok((id, client_rx))
    }

    /// Detach a client (its socket closed) and signal the host with
    /// [`HostEvent::Leave`] so it tears down that client's subscription.
    /// Idempotent.
    pub fn detach_client(&self, session: &str, client_id: u64) {
        if let Some(conn) = self.sessions.lock().unwrap().get_mut(session) {
            if conn.clients.remove(&client_id).is_some() {
                let _ = conn.host_tx.send(HostEvent::Leave { client: client_id });
            }
        }
    }

    /// Forward a frame a client sent to the host as [`HostEvent::Msg`]. Returns
    /// false if there's no live host (the client should disconnect). Binary client
    /// frames are dropped — review commands are text.
    pub fn from_client(&self, session: &str, client_id: u64, frame: Frame) -> bool {
        let Frame::Text(data) = frame else {
            return true; // ignore non-text client frames
        };
        let sessions = self.sessions.lock().unwrap();
        match sessions.get(session) {
            Some(conn) => conn
                .host_tx
                .send(HostEvent::Msg {
                    client: client_id,
                    data,
                })
                .is_ok(),
            None => false,
        }
    }

    /// Route a frame the host addressed to one client. No-op if that client (or
    /// the session) is gone.
    pub fn to_client(&self, session: &str, client_id: u64, frame: Frame) {
        if let Some(conn) = self.sessions.lock().unwrap().get(session) {
            if let Some(tx) = conn.clients.get(&client_id) {
                let _ = tx.send(frame);
            }
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
/// runs + tests — never wire this in production (it trusts any caller).
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

/// `GET /relay/host/{session}` — the host parks its outbound socket here.
#[cfg(not(tarpaulin_include))] // WS upgrade + socket loop — the Relay hub is unit-tested
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
#[cfg(not(tarpaulin_include))] // WS upgrade + socket loop — the Relay hub is unit-tested
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

/// Drive a host socket: register it, deliver client events as JSON, and route the
/// host's `to{client,data}` commands to the addressed client. Tears the session
/// down when the socket closes.
#[cfg(not(tarpaulin_include))] // socket I/O loop
async fn run_host(relay: Arc<Relay>, session: String, owner: String, socket: WebSocket) {
    let Some(mut host_rx) = relay.register_host(&session, &owner) else {
        return; // a host already holds this session
    };
    let (mut sink, mut stream) = socket.split();
    let mut writer = tokio::spawn(async move {
        while let Some(event) = host_rx.recv().await {
            let json = match serde_json::to_string(&event) {
                Ok(j) => j,
                Err(_) => continue,
            };
            if sink.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });
    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(t))) => {
                    if let Ok(HostCmd::To { client, data }) = serde_json::from_str::<HostCmd>(&t) {
                        relay.to_client(&session, client, Frame::Text(data));
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => { /* host speaks JSON envelopes only */ }
            },
            _ = &mut writer => break,
        }
    }
    relay.drop_host(&session);
    writer.abort();
}

/// Drive a client socket: attach it (signalling the host to open its
/// subscription), forward its frames to the host as commands, and write the
/// host's addressed frames back. Detaches on close.
#[cfg(not(tarpaulin_include))] // socket I/O loop
async fn run_client(relay: Arc<Relay>, session: String, owner: String, socket: WebSocket) {
    let (id, mut client_rx) = match relay.attach_client(&session, &owner) {
        Ok(pair) => pair,
        Err(_) => return, // no host / forbidden — drop the upgrade
    };
    let (mut sink, mut stream) = socket.split();
    let mut writer = tokio::spawn(async move {
        while let Some(frame) = client_rx.recv().await {
            let msg = match frame {
                Frame::Text(t) => Message::Text(t.into()),
                Frame::Binary(b) => Message::Binary(b.into()),
            };
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });
    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(t))) => {
                    if !relay.from_client(&session, id, Frame::Text(t.to_string())) {
                        break; // host gone
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => { /* clients send text commands */ }
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
    fn host_registers_once_and_attach_signals_join() {
        let relay = Relay::new();
        assert!(!relay.has_host("s1"));

        let mut host_rx = relay.register_host("s1", "acct-a").expect("first host registers");
        assert!(relay.has_host("s1"));
        assert!(relay.register_host("s1", "acct-a").is_none(), "one host per session");

        // Owner match attaches AND the host is told a client joined.
        let (cid, _crx) = relay.attach_client("s1", "acct-a").expect("owner attaches");
        assert_eq!(host_rx.try_recv().unwrap(), HostEvent::Join { client: cid });
        assert_eq!(relay.client_count("s1"), 1);

        // Mismatched owner is forbidden; absent session is NoHost.
        assert!(matches!(
            relay.attach_client("s1", "acct-b"),
            Err(AttachError::Forbidden)
        ));
        assert!(matches!(
            relay.attach_client("ghost", "acct-a"),
            Err(AttachError::NoHost)
        ));

        // Detach signals leave.
        relay.detach_client("s1", cid);
        assert_eq!(host_rx.try_recv().unwrap(), HostEvent::Leave { client: cid });
        assert_eq!(relay.client_count("s1"), 0);
    }

    #[test]
    fn client_frames_reach_the_host_tagged_with_the_client_id() {
        let relay = Relay::new();
        let mut host_rx = relay.register_host("s", "acct").unwrap();
        let (cid, _crx) = relay.attach_client("s", "acct").unwrap();
        assert_eq!(host_rx.try_recv().unwrap(), HostEvent::Join { client: cid });

        assert!(relay.from_client("s", cid, Frame::Text("answer".into())));
        assert_eq!(
            host_rx.try_recv().unwrap(),
            HostEvent::Msg { client: cid, data: "answer".into() }
        );

        // No host → nowhere to go.
        relay.drop_host("s");
        assert!(!relay.from_client("s", cid, Frame::Text("late".into())));
    }

    #[test]
    fn host_routes_a_frame_to_one_client_only() {
        let relay = Relay::new();
        let _host_rx = relay.register_host("s", "acct").unwrap();
        let (c1, mut rx1) = relay.attach_client("s", "acct").unwrap();
        let (_c2, mut rx2) = relay.attach_client("s", "acct").unwrap();

        // A frame addressed to c1 reaches ONLY c1 — each client has its own
        // subscription (snapshot-on-connect), not a broadcast.
        relay.to_client("s", c1, Frame::Text("snapshot-for-1".into()));
        assert_eq!(rx1.try_recv().unwrap(), Frame::Text("snapshot-for-1".into()));
        assert!(rx2.try_recv().is_err());
    }

    #[test]
    fn dropping_the_host_closes_client_receivers() {
        let relay = Relay::new();
        let _host_rx = relay.register_host("s", "acct").unwrap();
        let (_c, mut crx) = relay.attach_client("s", "acct").unwrap();

        relay.drop_host("s");
        assert!(crx.try_recv().is_err());
        assert!(!relay.has_host("s"));
    }

    // End-to-end over real sockets: a client attach signals join to the host, the
    // host addresses a snapshot back to that client, and a client command reaches
    // the host tagged with the id.
    #[tokio::test]
    async fn bridges_per_client_end_to_end_over_websockets() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as T;

        let relay = Arc::new(Relay::new());
        let state = RelayState::new(relay.clone(), Arc::new(DevTokenAuth));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, relay_router(state)).await.unwrap();
        });

        let (mut host, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}/relay/host/s1?token=acct"))
                .await
                .expect("host connects");
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

        // The host receives a join event carrying the client id.
        let join = host.next().await.unwrap().unwrap();
        let evt: HostEvent = serde_json::from_str(join.to_text().unwrap()).unwrap();
        let client_id = match evt {
            HostEvent::Join { client } => client,
            other => panic!("expected join, got {other:?}"),
        };

        // Host addresses a snapshot back to that client → only that client gets it.
        let to = serde_json::to_string(&HostCmd::To {
            client: client_id,
            data: "snapshot".into(),
        })
        .unwrap();
        host.send(T::Text(to.into())).await.unwrap();
        let got = client.next().await.unwrap().unwrap();
        assert_eq!(got.into_text().unwrap().as_str(), "snapshot");

        // A client command reaches the host as a tagged msg.
        client.send(T::Text("advance".into())).await.unwrap();
        let msg = host.next().await.unwrap().unwrap();
        let evt: HostEvent = serde_json::from_str(msg.to_text().unwrap()).unwrap();
        assert_eq!(evt, HostEvent::Msg { client: client_id, data: "advance".into() });
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

        let (_intruder, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}/relay/client/s1?token=owner-b"))
                .await
                .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(relay.client_count("s1"), 0, "wrong-owner client must not attach");
    }
}
