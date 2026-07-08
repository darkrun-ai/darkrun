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
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

// The relay routing envelope is the SHARED tunnel contract — one source of truth
// in darkrun-api, spoken by the relay, the host connector, and every client.
pub use darkrun_api::tunnel::{HostCmd, HostEvent};

use crate::push::{DeviceRegistry, InMemoryDeviceRegistry, NoopPushSender, PushSender};
use crate::relay_bus::{BusFrame, FrameBus};
use crate::relay_registry::SessionRegistry;

/// How often a live host renews its session doc's `expiresAt` in the shared
/// [`SessionRegistry`]. Comfortably under
/// [`SESSION_TTL`](crate::relay_registry::SESSION_TTL) so a missed beat or two
/// doesn't expire a live session.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// The public relay base URL the descriptor API hands clients — the same
/// `wss://relay.darkrun.ai` the engine dials by default (see
/// `darkrun_mcp::server::DEFAULT_RELAY_URL`). Overridable via
/// `DARKRUN_RELAY_PUBLIC_URL` for dev/staging.
pub const DEFAULT_RELAY_PUBLIC_URL: &str = "wss://relay.darkrun.ai";

/// The app host whose `/runs/{slug}` route the descriptor targets and the push
/// click URL opens (the universal link the native apps claim; the web app is the
/// fallback). Matches `darkrun_mcp`'s `APP_BASE_URL`.
pub(crate) const APP_BASE_URL: &str = "https://app.darkrun.ai";

/// One opaque review frame shuttled across the bridge. The relay never parses the
/// payload — both kinds are carried verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// A UTF-8 text frame (the review protocol's JSON messages).
    Text(String),
    /// A binary frame.
    Binary(Vec<u8>),
}

/// The live state of one tunnelled session hosted on THIS instance: the host's
/// event sink, its local clients' frame sinks, and the routing table for clients
/// attached on OTHER instances.
struct SessionConn {
    /// The account that owns this session; a client must resolve to the same
    /// owner to attach. Authoritative for the in-memory (no-registry) path. With a
    /// shared [`SessionRegistry`] wired, authz is sourced from Firestore, but this
    /// field is still load-bearing: it guards against attaching a client to a
    /// same-session-id host owned by a DIFFERENT account (owner-scoped docs let two
    /// accounts hold the same session id), and it addresses the owner-scoped doc to
    /// delete on host drop.
    owner: String,
    /// Push here to deliver a [`HostEvent`] to the host.
    host_tx: mpsc::UnboundedSender<HostEvent>,
    /// Per-LOCAL-client frame sinks, keyed by client id — clients that attached on
    /// THIS instance (the L1 fast path: the host reaches them with 0 bus hops).
    clients: HashMap<u64, mpsc::UnboundedSender<Frame>>,
    /// Client→instance routing table for clients attached on ANOTHER instance:
    /// built from the [`HostEvent::Join`] a bus frame carries, so a host frame
    /// addressed to such a client is published to the instance that holds its sink.
    /// The fast path; the `clients/{clientId}` registry sub-doc is the backstop.
    client_instances: HashMap<u64, String>,
    /// The heartbeat task renewing this session's registry doc, if a shared
    /// [`SessionRegistry`] is wired. Aborted when the host drops.
    heartbeat: Option<JoinHandle<()>>,
}

/// A client attached on THIS instance whose HOST is on another instance: its
/// frame sink lives here, but its [`HostEvent`]s (join/msg/leave) are published
/// over the [`FrameBus`] to the host's instance.
struct RemoteAttach {
    /// The client's frame sink — a bus `ToClient` frame delivers here.
    sink: mpsc::UnboundedSender<Frame>,
    /// The instance the host is on — where this client's events are published.
    host_instance: String,
    /// The session's owner, for addressing the routing backstop sub-doc on detach.
    owner: String,
}

/// Why an attach was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachError {
    /// No host is registered for that session (nothing live to reach).
    NoHost,
    /// The client's owner doesn't match the session's owner.
    Forbidden,
}

/// The bridge hub. Frame delivery has an in-memory L1 fast path (`session_id ->
/// SessionConn` for sessions hosted here; a per-instance `remote_clients` map for
/// clients attached here to a remotely-hosted session). With a shared
/// [`SessionRegistry`] wired, the authoritative session metadata + owner authz
/// live in Firestore so single-host-per-session and owner checks are correct
/// across Cloud Run instances.
///
/// **Cross-instance routing (Step 1c).** When a [`FrameBus`] is wired, a host on
/// instance A and a client on instance B exchange frames over the bus: the local
/// map stays the L1 fast path (host+client co-located → 0 bus hops), and a
/// split pair routes by `instance_id` (`to_client` publishes to the client's
/// instance; a client's join/msg/leave publish to `hostInstance`). With NO bus
/// (`None`, the single-instance/dev default), a client whose host is on another
/// instance is authorized by the registry but attach is `NoHost` — EXACTLY the
/// behavior before this landing.
pub struct Relay {
    sessions: Mutex<HashMap<String, SessionConn>>,
    /// Clients attached on THIS instance whose host is on another instance, keyed
    /// by `(session, client_id)`. Their sinks live here (a bus `ToClient` frame
    /// delivers into them); their events publish to the host's instance.
    remote_clients: Mutex<HashMap<(String, u64), RemoteAttach>>,
    next_client_id: AtomicU64,
    /// This process's instance id, unique for its lifetime. It routes frames
    /// across instances (Step 1c) and fixes `hostInstance` in the session doc so
    /// heartbeat / drop only touch a doc THIS instance still owns.
    instance_id: String,
    /// The cross-instance session registry (Firestore). `None` → today's pure
    /// in-memory behavior (dev/tests): authz + single-host come off the local map.
    /// `Some` → session metadata + authz go through Firestore.
    shared: Option<Arc<dyn SessionRegistry>>,
    /// The cross-instance frame bus (Pub/Sub). `None` → single-instance, local
    /// delivery only (unchanged from before Step 1c). `Some` → a split host/client
    /// pair exchange frames through it.
    bus: Option<Arc<dyn FrameBus>>,
}

impl Default for Relay {
    fn default() -> Self {
        Self::new()
    }
}

impl Relay {
    /// A fresh, empty relay with no shared registry and no frame bus — pure
    /// in-memory routing + authz (dev/tests, and single-instance production).
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            remote_clients: Mutex::new(HashMap::new()),
            next_client_id: AtomicU64::new(0),
            instance_id: new_instance_id(),
            shared: None,
            bus: None,
        }
    }

    /// Wire a cross-instance [`SessionRegistry`] (Firestore): session metadata +
    /// single-host-per-session + owner authz go through it. Returns `self` for
    /// chaining off [`new`](Self::new).
    pub fn with_registry(mut self, shared: Arc<dyn SessionRegistry>) -> Self {
        self.shared = Some(shared);
        self
    }

    /// Wire the cross-instance [`FrameBus`] (Pub/Sub): a host and a client on
    /// different instances exchange frames through it. Absent, the relay is
    /// single-instance (a split pair can't exchange frames). Returns `self` for
    /// chaining off [`new`](Self::new).
    pub fn with_bus(mut self, bus: Arc<dyn FrameBus>) -> Self {
        self.bus = Some(bus);
        self
    }

    /// This process's instance id (unique for its lifetime). Used by Step 1c
    /// frame routing; exposed for observability.
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// Register `session` as hosted by `owner`, returning the receiver the host's
    /// writer task drains ([`HostEvent`]s bound for the host). Refuses (`None`) if
    /// a host is already registered — one host per session.
    ///
    /// With a shared [`SessionRegistry`] wired, the single-host guarantee is a
    /// Firestore CAS (create-if-absent or take over a stale doc), correct across
    /// instances, and a heartbeat task renews the doc until the host drops. Absent
    /// a registry, it's today's first-host-wins on the local map.
    pub async fn register_host(
        &self,
        session: &str,
        owner: &str,
    ) -> Option<mpsc::UnboundedReceiver<HostEvent>> {
        // Cross-instance claim first (no lock held across the await). Losing the
        // CAS — a live host already holds it, or the backend was unreachable —
        // means we don't register; the host socket reconnects and retries.
        let heartbeat = if let Some(shared) = &self.shared {
            if !shared.register_host(session, owner, &self.instance_id).await {
                return None;
            }
            Some(self.spawn_heartbeat(session.to_string(), owner.to_string()))
        } else {
            None
        };
        // Claim the local map entry. Scope the lock so it never spans the drop_host
        // await below (the guard is `!Send`, and we must not hold it across `.await`).
        {
            let mut sessions = self.sessions.lock().unwrap();
            if !sessions.contains_key(session) {
                let (host_tx, host_rx) = mpsc::unbounded_channel();
                sessions.insert(
                    session.to_string(),
                    SessionConn {
                        owner: owner.to_string(),
                        host_tx,
                        clients: HashMap::new(),
                        client_instances: HashMap::new(),
                        heartbeat,
                    },
                );
                return Some(host_rx);
            }
        }
        // In-memory: one host per session id on this instance. In the shared path
        // we already won the CAS, so a local entry here can't happen in practice
        // (register runs once per host socket) — but if it did, unwind cleanly:
        // abort the just-spawned heartbeat AND drop the just-created shared doc so
        // it isn't stranded until SESSION_TTL, then refuse rather than clobber the
        // live host.
        if let Some(hb) = heartbeat {
            hb.abort();
        }
        if let Some(shared) = &self.shared {
            shared.drop_host(session, owner, &self.instance_id).await;
        }
        None
    }

    /// Spawn the heartbeat task that renews the `(owner, session)` registry doc
    /// every [`HEARTBEAT_INTERVAL`] until aborted (on host drop). Only called when
    /// a shared registry is wired. The `owner` addresses the owner-scoped doc, so
    /// the heartbeat renews (and self-heals) exactly this host's own doc.
    fn spawn_heartbeat(&self, session: String, owner: String) -> JoinHandle<()> {
        let shared = self
            .shared
            .clone()
            .expect("spawn_heartbeat is only reached with a shared registry");
        let instance = self.instance_id.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(HEARTBEAT_INTERVAL);
            tick.tick().await; // the interval's first tick is immediate — skip it
            loop {
                tick.tick().await;
                shared.heartbeat(&session, &owner, &instance).await;
            }
        })
    }

    /// Tear down a session when its host disconnects: drop the local entry so
    /// every attached client's sender closes (ending their writer tasks) and a
    /// later attach sees `NoHost`, stop its heartbeat, and delete the session doc
    /// (only if THIS instance still owns it; native TTL + the read-time expiry
    /// check are the backstop if the delete is lost).
    pub async fn drop_host(&self, session: &str) {
        // Take the local entry (releasing the lock before any await) — its `owner`
        // addresses the owner-scoped registry doc to delete.
        let removed = self.sessions.lock().unwrap().remove(session);
        let Some(conn) = removed else {
            return; // already gone — nothing local to tear down (and no owner to
                    // address the shared doc; the drop that removed it dropped it).
        };
        if let Some(hb) = conn.heartbeat {
            hb.abort();
        }
        if let Some(shared) = &self.shared {
            shared.drop_host(session, &conn.owner, &self.instance_id).await;
        }
    }

    /// Attach a client to `session` (owned by `owner`), returning `(client_id,
    /// receiver)` — the receiver feeds the client's writer task. Also signals the
    /// host with [`HostEvent::Join`] so it opens this client's subscription.
    /// Refuses if no host is live (`NoHost`) or the owner mismatches (`Forbidden`).
    ///
    /// With a shared [`SessionRegistry`], authz + host location read the
    /// owner-scoped Firestore doc for THIS client's own `(owner, session)` (a
    /// client may hit a different instance than the host). A live doc authorizes
    /// and yields the `hostInstance`; a missing/expired one is `NoHost`. There is
    /// no cross-owner `Forbidden` in this path: a different owner is a different
    /// doc, so it simply doesn't exist for this client and reads as `NoHost`. Then:
    ///
    /// * **host on THIS instance** (local `SessionConn`, owner matches) → attach
    ///   into the local map, 0 bus hops (the L1 fast path);
    /// * **host on ANOTHER instance** → with a [`FrameBus`] wired, register the
    ///   client's sink locally and publish its `Join` to `hostInstance` (writing
    ///   the routing backstop sub-doc); with NO bus it's `NoHost` (single-instance,
    ///   exactly the behavior before Step 1c).
    pub async fn attach_client(
        &self,
        session: &str,
        owner: &str,
    ) -> Result<(u64, mpsc::UnboundedReceiver<Frame>), AttachError> {
        if let Some(shared) = &self.shared {
            // Authz + host location from Firestore for THIS client's owner (no lock
            // across the await). Owner-scoped doc: present+live → the hostInstance,
            // else NoHost.
            let Some(host_instance) = shared.lookup_host_instance(session, owner).await else {
                return Err(AttachError::NoHost);
            };
            // Fast path: the host is on THIS instance's local map (owner matches).
            // A conn for a DIFFERENT owner (a same-slug session owned by another
            // account) means this client's real host is elsewhere — fall through.
            {
                let mut sessions = self.sessions.lock().unwrap();
                if let Some(conn) = sessions.get_mut(session) {
                    if conn.owner == owner {
                        return Ok(self.attach_to(conn));
                    }
                }
            }
            // Host on another instance. Reaching it needs the frame bus; absent, a
            // split pair can't exchange frames → NoHost (single-instance behavior).
            let Some(bus) = &self.bus else {
                return Err(AttachError::NoHost);
            };
            // The doc names THIS instance but there's no local host for it — a stale
            // self-pointer (the host dropped without clearing the doc); not live.
            if host_instance == self.instance_id {
                return Err(AttachError::NoHost);
            }
            let (id, client_rx) = self.attach_remote(session, owner, &host_instance);
            // Durable routing backstop (best-effort) + signal Join to the host's
            // instance over the bus.
            shared.register_client(session, owner, id, &self.instance_id).await;
            bus.publish(
                &host_instance,
                session,
                BusFrame::to_host(owner, &self.instance_id, HostEvent::Join { client: id }),
            )
            .await;
            return Ok((id, client_rx));
        }
        // In-memory: owner authz off the local map's owner field.
        let mut sessions = self.sessions.lock().unwrap();
        let conn = sessions.get_mut(session).ok_or(AttachError::NoHost)?;
        if conn.owner != owner {
            return Err(AttachError::Forbidden);
        }
        Ok(self.attach_to(conn))
    }

    /// Mint a client id, register its frame sink on `conn`, and signal the host
    /// that a client joined. The map-mutation half shared by both authz paths.
    fn attach_to(&self, conn: &mut SessionConn) -> (u64, mpsc::UnboundedReceiver<Frame>) {
        let id = self.mint_client_id();
        let (client_tx, client_rx) = mpsc::unbounded_channel();
        conn.clients.insert(id, client_tx);
        // Tell the host a client joined (best-effort; host may be mid-teardown).
        let _ = conn.host_tx.send(HostEvent::Join { client: id });
        (id, client_rx)
    }

    /// Register a client whose HOST is on `host_instance` (not here): its sink
    /// lives on this instance (in `remote_clients`), its events publish to the
    /// host's instance. The cross-instance analogue of [`attach_to`](Self::attach_to).
    fn attach_remote(
        &self,
        session: &str,
        owner: &str,
        host_instance: &str,
    ) -> (u64, mpsc::UnboundedReceiver<Frame>) {
        let id = self.mint_client_id();
        let (client_tx, client_rx) = mpsc::unbounded_channel();
        self.remote_clients.lock().unwrap().insert(
            (session.to_string(), id),
            RemoteAttach {
                sink: client_tx,
                host_instance: host_instance.to_string(),
                owner: owner.to_string(),
            },
        );
        (id, client_rx)
    }

    /// Mint a globally-unique client id. Sequential per instance, but hashed with
    /// the instance id so two instances never mint the same id for the same
    /// session — global uniqueness comes from the id + the client→instance table,
    /// NOT from encoding the instance into a decodable id.
    fn mint_client_id(&self) -> u64 {
        let n = self.next_client_id.fetch_add(1, Ordering::Relaxed);
        let mut hasher = Sha256::new();
        hasher.update(self.instance_id.as_bytes());
        hasher.update(n.to_le_bytes());
        let digest = hasher.finalize();
        u64::from_le_bytes(digest[..8].try_into().expect("sha256 yields >= 8 bytes"))
    }

    /// Detach a client (its socket closed) and signal the host with
    /// [`HostEvent::Leave`] so it tears down that client's subscription.
    /// Idempotent. If the host is on another instance the Leave is published over
    /// the bus (and the routing backstop sub-doc is dropped).
    pub async fn detach_client(&self, session: &str, client_id: u64) {
        // Local client (host co-located) → signal the local host directly.
        {
            let mut sessions = self.sessions.lock().unwrap();
            if let Some(conn) = sessions.get_mut(session) {
                if conn.clients.remove(&client_id).is_some() {
                    let _ = conn.host_tx.send(HostEvent::Leave { client: client_id });
                    return;
                }
            }
        }
        // Remote client (host on another instance) → publish Leave to it.
        let removed = self
            .remote_clients
            .lock()
            .unwrap()
            .remove(&(session.to_string(), client_id));
        if let Some(attach) = removed {
            if let Some(bus) = &self.bus {
                bus.publish(
                    &attach.host_instance,
                    session,
                    BusFrame::to_host(
                        &attach.owner,
                        &self.instance_id,
                        HostEvent::Leave { client: client_id },
                    ),
                )
                .await;
            }
            if let Some(shared) = &self.shared {
                shared
                    .drop_client(session, &attach.owner, client_id, &self.instance_id)
                    .await;
            }
        }
    }

    /// Forward a frame a client sent to the host as [`HostEvent::Msg`]. Returns
    /// false if there's no live host (the client should disconnect). Binary client
    /// frames are dropped — review commands are text.
    ///
    /// A client attached to a remotely-hosted session (its sink is in
    /// `remote_clients`) publishes its Msg over the bus to `hostInstance`; a
    /// co-located client delivers straight to the local host. Absent a bus/registry
    /// no remote clients exist, so this is exactly the local path as before.
    pub async fn from_client(&self, session: &str, client_id: u64, frame: Frame) -> bool {
        let Frame::Text(data) = frame else {
            return true; // ignore non-text client frames
        };
        // Remote-attached client → publish to the host's instance over the bus,
        // tagged with the attach's own owner so the host's instance can enforce it.
        let routing = self
            .remote_clients
            .lock()
            .unwrap()
            .get(&(session.to_string(), client_id))
            .map(|a| (a.host_instance.clone(), a.owner.clone()));
        if let Some((host_instance, owner)) = routing {
            return match &self.bus {
                Some(bus) => {
                    bus.publish(
                        &host_instance,
                        session,
                        BusFrame::to_host(
                            &owner,
                            &self.instance_id,
                            HostEvent::Msg { client: client_id, data },
                        ),
                    )
                    .await;
                    true
                }
                None => false,
            };
        }
        // Co-located client → deliver straight to the local host.
        let sessions = self.sessions.lock().unwrap();
        match sessions.get(session) {
            Some(conn) => conn
                .host_tx
                .send(HostEvent::Msg { client: client_id, data })
                .is_ok(),
            None => false,
        }
    }

    /// Route a frame the host addressed to one client. If the client's sink is on
    /// THIS instance (co-located) it's delivered locally (0 bus hops). Otherwise
    /// the client's instance is resolved (in-memory routing table → registry
    /// backstop) and the frame is published there. No-op if the client is unknown.
    pub async fn to_client(&self, session: &str, client_id: u64, frame: Frame) {
        // L1 fast path: the client's sink is local to this (host) instance.
        {
            let sessions = self.sessions.lock().unwrap();
            if let Some(conn) = sessions.get(session) {
                if let Some(tx) = conn.clients.get(&client_id) {
                    let _ = tx.send(frame);
                    return;
                }
            }
        }
        // Resolve the client's instance: routing table (fast) then sub-doc backstop.
        // The session's owner tags the published frame so the receiving instance
        // enforces owner at delivery (a cross-owner frame is dropped there).
        let (routed, owner) = {
            let sessions = self.sessions.lock().unwrap();
            match sessions.get(session) {
                Some(conn) => (
                    conn.client_instances.get(&client_id).cloned(),
                    Some(conn.owner.clone()),
                ),
                None => (None, None),
            }
        };
        // No local host for this session → no owner to tag and nothing to route.
        let Some(owner) = owner else {
            return;
        };
        let instance = match routed {
            Some(instance) => Some(instance),
            None => match &self.shared {
                Some(shared) => shared.lookup_client_instance(session, &owner, client_id).await,
                None => None,
            },
        };
        if let (Some(instance), Some(bus), Frame::Text(data)) = (instance, &self.bus, frame) {
            bus.publish(&instance, session, BusFrame::to_client(&owner, client_id, data))
                .await;
        }
    }

    /// Deliver a frame that arrived over the [`FrameBus`] into the SAME local
    /// delivery path an in-process frame uses — the single re-entry point the
    /// subscriber calls, so bus and in-process frames share one path. The frame's
    /// authenticated `owner` is carried through to delivery and ENFORCED there
    /// against the local host's / remote-client's owner (a mismatch is dropped), so
    /// a colliding session slug across two owners can never cross the owner
    /// boundary — matching the registry's owner-scoped keying.
    pub fn deliver_bus_frame(&self, session: &str, frame: BusFrame) {
        match frame {
            BusFrame::ToClient { owner, client, data } => {
                self.deliver_client_frame(session, &owner, client, Frame::Text(data))
            }
            BusFrame::ToHost { owner, from_instance, event } => {
                self.deliver_host_event(session, &owner, &from_instance, event)
            }
        }
    }

    /// Local delivery of a host-addressed frame to a client whose sink lives on
    /// THIS instance (it attached here to a remotely-hosted session). DROPS the
    /// frame unless the local remote-client attach's owner matches `owner` — a
    /// cross-owner frame is never delivered and never falls through. Falls back to
    /// a co-located client (of the same owner) for robustness.
    fn deliver_client_frame(&self, session: &str, owner: &str, client_id: u64, frame: Frame) {
        {
            let remote = self.remote_clients.lock().unwrap();
            if let Some(attach) = remote.get(&(session.to_string(), client_id)) {
                // Owner gate: only the attach's own owner may reach its sink.
                if attach.owner == owner {
                    let _ = attach.sink.send(frame);
                }
                return; // known client id — never fall through on a mismatch
            }
        }
        // Co-located fallback: deliver only to a local host of the SAME owner.
        if let Some(conn) = self.sessions.lock().unwrap().get(session) {
            if conn.owner == owner {
                if let Some(tx) = conn.clients.get(&client_id) {
                    let _ = tx.send(frame);
                }
            }
        }
    }

    /// Local delivery of a client event to the host on THIS instance. DROPS the
    /// frame (no routing-table write, no forward to the host) unless the local
    /// host's owner matches `owner` — so a bus frame for a DIFFERENT owner that
    /// shares this session slug can never reach this host. A `Join` records the
    /// client's origin instance in the routing table (so the host can address it
    /// later); a `Leave` clears it; then the event is handed to the host. No-op if
    /// this instance no longer hosts the session.
    fn deliver_host_event(&self, session: &str, owner: &str, from_instance: &str, event: HostEvent) {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(conn) = sessions.get_mut(session) else {
            return;
        };
        // Owner gate: a frame for another owner's same-slug session is dropped
        // before it can touch the routing table or reach the host.
        if conn.owner != owner {
            return;
        }
        match &event {
            HostEvent::Join { client } => {
                conn.client_instances.insert(*client, from_instance.to_string());
            }
            HostEvent::Leave { client } => {
                conn.client_instances.remove(client);
            }
            HostEvent::Msg { .. } => {}
        }
        let _ = conn.host_tx.send(event);
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

    /// Whether a host is registered for `session` on THIS instance's local map.
    /// (Cross-instance liveness lives in the [`SessionRegistry`]; this is the
    /// local-routing view, which is all the frame path uses this landing.)
    pub fn has_host(&self, session: &str) -> bool {
        self.sessions.lock().unwrap().contains_key(session)
    }

    /// Whether a LIVE host owned by `owner` holds `session` — the reachability
    /// check behind the descriptor API (`GET /api/runs/{slug}/relay`). It reuses
    /// the exact owner-scoping the attach path enforces:
    ///
    /// * with a shared [`SessionRegistry`] → read the owner-scoped
    ///   `(owner, session)` doc (the cross-instance source of truth); a live doc
    ///   means reachable, a missing/expired one means not;
    /// * absent a registry (single-instance/dev) → the local map's `owner` field,
    ///   so a session hosted here by a DIFFERENT account never reads as reachable.
    ///
    /// Owner-match is the security boundary: this never reports a run the caller
    /// doesn't own as reachable.
    pub async fn host_owned_by(&self, session: &str, owner: &str) -> bool {
        if let Some(shared) = &self.shared {
            return shared.lookup_owner(session, owner).await.is_some();
        }
        self.sessions
            .lock()
            .unwrap()
            .get(session)
            .map(|conn| conn.owner == owner)
            .unwrap_or(false)
    }
}

/// A per-process instance id, unique for this process's lifetime.
///
/// Cloud Run has no stable instance id, but a process IS one instance's life, and
/// the routing this feeds (Step 1c) only needs to tell concurrently-live instances
/// apart. Derived from the process's start-nanos + pid, hashed to a short hex
/// token — no `uuid` dependency, and hashing ids to hex is the same idiom the
/// Firestore document ids use.
fn new_instance_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(pid.to_le_bytes());
    hasher.finalize().iter().take(16).map(|b| format!("{b:02x}")).collect()
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

/// The relay router's shared state: the bridge hub, the token verifier, and the
/// remote-push pair (device registry + sender).
#[derive(Clone)]
pub struct RelayState {
    relay: Arc<Relay>,
    auth: Arc<dyn RelayAuth>,
    devices: Arc<dyn DeviceRegistry>,
    push: Arc<dyn PushSender>,
    /// The public relay base URL the descriptor API echoes to clients (e.g.
    /// `wss://relay.darkrun.ai`). Defaults to [`DEFAULT_RELAY_PUBLIC_URL`].
    relay_public_url: String,
}

impl RelayState {
    /// Build relay state from a hub + a token verifier, with remote push
    /// **disabled** — an in-memory registry plus a no-op sender. The host's
    /// local OS notification still fires; only the remote (FCM) half is dark
    /// until [`with_push`](Self::with_push) wires it. Safe default for dev/tests.
    /// The public relay URL defaults to [`DEFAULT_RELAY_PUBLIC_URL`].
    pub fn new(relay: Arc<Relay>, auth: Arc<dyn RelayAuth>) -> Self {
        Self {
            relay,
            auth,
            devices: Arc::new(InMemoryDeviceRegistry::new()),
            push: Arc::new(NoopPushSender),
            relay_public_url: DEFAULT_RELAY_PUBLIC_URL.to_string(),
        }
    }

    /// Wire the device registry + push sender for remote notifications (FCM in
    /// production). Returns `self` for chaining off [`new`](Self::new).
    pub fn with_push(
        mut self,
        devices: Arc<dyn DeviceRegistry>,
        push: Arc<dyn PushSender>,
    ) -> Self {
        self.devices = devices;
        self.push = push;
        self
    }

    /// Set the public relay base URL the descriptor API hands clients (the same
    /// `wss://…` base the engine dials). Returns `self` for chaining.
    pub fn with_relay_url(mut self, relay_public_url: impl Into<String>) -> Self {
        self.relay_public_url = relay_public_url.into().trim_end_matches('/').to_string();
        self
    }

    /// Fan a notification out to `owner`'s registered devices — the relay's
    /// [`HostCmd::Notify`] handling. `link` is the run's web view, threaded into
    /// the push so tapping the notification opens the live run. Returns how many
    /// devices were pushed.
    pub async fn notify_owner(
        &self,
        owner: &str,
        title: &str,
        body: &str,
        link: Option<&str>,
    ) -> usize {
        crate::push::fan_out(&self.devices, &self.push, owner, title, body, link).await
    }

    /// Resolve a bearer token to its account via the configured verifier.
    fn account_for(&self, token: &str) -> Option<String> {
        self.auth.account_for(token)
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

/// A device registering for remote push: its FCM token + platform. The owning
/// account is taken from the verified bearer token, never the body.
#[derive(Deserialize)]
pub struct RegisterDevice {
    /// The FCM registration token to push to.
    pub token: String,
    /// The platform (`ios`/`android`/`web`/`macos`).
    pub platform: String,
}

/// Origins allowed to call `/devices` cross-origin. The web app lives on
/// `app.darkrun.ai` but the relay (and `/devices`) is served from the website
/// host (`darkrun.ai`), so the browser does a CORS preflight; `localhost` covers
/// local `dx serve`.
pub(crate) const APP_ORIGINS: &[&str] = &["https://app.darkrun.ai", "http://localhost:8080"];

/// Mount the device-registration endpoints, keyed off the same Firebase token
/// the relay authenticates with:
/// - `POST /devices` — register/refresh a device for the caller's account;
/// - `DELETE /devices/{token}` — drop a device (logout / token rotation).
///
/// CORS is allowed for the web-app origins (cross-origin: app.darkrun.ai →
/// darkrun.ai), permitting the `Authorization` + `Content-Type` headers the
/// registration request carries.
pub fn device_router(state: RelayState) -> Router {
    use axum::http::{header, HeaderValue, Method};
    use tower_http::cors::CorsLayer;

    let origins: Vec<HeaderValue> = APP_ORIGINS
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::POST, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    Router::new()
        .route("/devices", axum::routing::post(register_device))
        .route("/devices/{token}", axum::routing::delete(unregister_device))
        .layer(cors)
        .with_state(state)
}

/// The relay-attach descriptor the web client needs to reach a live run — the
/// CLEAN-link resolution behind `GET /api/runs/{slug}/relay`. It carries NO
/// secret in the URL: the client fetches this from an AUTHENTICATED endpoint,
/// then assembles `{relay_url}/relay/client/{session}?token={client_token}`.
#[derive(Debug, Serialize)]
pub struct RelayDescriptor {
    /// The public relay base URL (e.g. `wss://relay.darkrun.ai`).
    pub relay_url: String,
    /// The relay session id to attach to — the run slug.
    pub session: String,
    /// The bearer token the client authenticates the relay attach with. The
    /// caller's own verified Firebase ID token, echoed back: the relay's
    /// owner-scoped attach already binds a client to a session owned by the same
    /// uid, so reusing the caller's token is safe (and nothing new leaks).
    pub client_token: String,
}

/// Mount the run-reachability descriptor endpoint:
/// - `GET /api/runs/{slug}/relay` — resolve the relay/session/token a web client
///   needs to reach the caller's live run.
///
/// CORS mirrors [`device_router`] (the web app on `app.darkrun.ai` calls this
/// cross-origin to the relay host), allowing the `Authorization` header on a
/// `GET`.
pub fn relay_descriptor_router(state: RelayState) -> Router {
    use axum::http::{header, HeaderValue, Method};
    use tower_http::cors::CorsLayer;

    let origins: Vec<HeaderValue> = APP_ORIGINS
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    Router::new()
        .route("/api/runs/{slug}/relay", get(run_relay_descriptor))
        .layer(cors)
        .with_state(state)
}

/// `GET /api/runs/{slug}/relay` — mint the relay-attach descriptor for a live
/// run the caller owns.
///
/// 1. verify the caller's Firebase ID token → `uid` (a missing/invalid token is
///    `401`);
/// 2. confirm a LIVE host owned by that `uid` holds `slug` in the session
///    registry ([`Relay::host_owned_by`]); if none, `404` — there's nothing to
///    reach, and owner-match is the security boundary (never a descriptor for a
///    run the caller doesn't own);
/// 3. return `{ relay_url, session, client_token }`, where `client_token` is the
///    caller's own verified token (the relay's owner-scoped attach enforces the
///    same-uid boundary, so reuse is safe).
async fn run_relay_descriptor(
    State(state): State<RelayState>,
    Path(slug): Path<String>,
    headers: axum::http::HeaderMap,
) -> Response {
    let Some(token) = bearer(&headers) else {
        return (StatusCode::UNAUTHORIZED, "a Firebase ID token is required").into_response();
    };
    let Some(uid) = state.account_for(token) else {
        return (StatusCode::UNAUTHORIZED, "invalid or expired token").into_response();
    };
    if !state.relay.host_owned_by(&slug, &uid).await {
        // No live host for (uid, slug): the run isn't reachable — or isn't the
        // caller's. Either way there's nothing to hand back.
        return (StatusCode::NOT_FOUND, "no live host for this run").into_response();
    }
    // The descriptor carries a bearer token, so it must never be cached by the
    // browser or any intermediary.
    (
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        axum::Json(RelayDescriptor {
            relay_url: state.relay_public_url.clone(),
            session: slug,
            client_token: token.to_string(),
        }),
    )
        .into_response()
}

/// Read the `Authorization: Bearer <token>` header, if present and well-formed.
fn bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// `POST /devices` — register the caller's device. The account comes from the
/// verified bearer token; a missing/invalid token is `401`.
async fn register_device(
    State(state): State<RelayState>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<RegisterDevice>,
) -> Response {
    let Some(account) = bearer(&headers).and_then(|t| state.account_for(t)) else {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    };
    state
        .devices
        .register(
            &account,
            crate::push::DeviceToken { token: body.token, platform: body.platform },
        )
        .await;
    StatusCode::NO_CONTENT.into_response()
}

/// `DELETE /devices/{token}` — drop a device token. Requires a valid bearer
/// token AND that the caller's account owns the token: without the ownership
/// check any authenticated account could unregister a stranger's device by its
/// token, silently disabling their gate push. Idempotent — a token the caller
/// doesn't own is a `204` no-op (no oracle for which tokens exist).
async fn unregister_device(
    State(state): State<RelayState>,
    headers: axum::http::HeaderMap,
    Path(token): Path<String>,
) -> Response {
    let Some(account) = bearer(&headers).and_then(|t| state.account_for(t)) else {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    };
    state.devices.unregister_for(&account, &token).await;
    StatusCode::NO_CONTENT.into_response()
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
    ws.on_upgrade(move |socket| run_host(state, session, owner, socket))
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
async fn run_host(state: RelayState, session: String, owner: String, socket: WebSocket) {
    let relay = state.relay.clone();
    let Some(mut host_rx) = relay.register_host(&session, &owner).await else {
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
                Some(Ok(Message::Text(t))) => match serde_json::from_str::<HostCmd>(&t) {
                    // Route a review frame to one client.
                    Ok(HostCmd::To { client, data }) => {
                        relay.to_client(&session, client, Frame::Text(data)).await;
                    }
                    // Fan a notification out to the owner's remote devices. The
                    // session id IS the run slug, so the click target is the run's
                    // web view — tapping the push opens the live run.
                    Ok(HostCmd::Notify { title, body }) => {
                        let link = format!("{APP_BASE_URL}/runs/{session}");
                        let n = state.notify_owner(&owner, &title, &body, Some(&link)).await;
                        tracing::debug!(session = %session, devices = n, "relayed push");
                    }
                    Err(_) => { /* not a known envelope — ignore */ }
                },
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => { /* host speaks JSON envelopes only */ }
            },
            _ = &mut writer => break,
        }
    }
    relay.drop_host(&session).await;
    writer.abort();
}

/// Drive a client socket: attach it (signalling the host to open its
/// subscription), forward its frames to the host as commands, and write the
/// host's addressed frames back. Detaches on close.
#[cfg(not(tarpaulin_include))] // socket I/O loop
async fn run_client(relay: Arc<Relay>, session: String, owner: String, socket: WebSocket) {
    let (id, mut client_rx) = match relay.attach_client(&session, &owner).await {
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
                    if !relay.from_client(&session, id, Frame::Text(t.to_string())).await {
                        break; // host gone
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => { /* clients send text commands */ }
            },
            _ = &mut writer => break,
        }
    }
    relay.detach_client(&session, id).await;
    writer.abort();
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::push::{DeviceToken, InMemoryDeviceRegistry, PushSender};

    #[test]
    fn dev_token_auth_resolves_token_to_account() {
        assert_eq!(DevTokenAuth.account_for("acct-a"), Some("acct-a".to_string()));
        assert_eq!(DevTokenAuth.account_for("  acct-b "), Some("acct-b".to_string()));
        assert_eq!(DevTokenAuth.account_for(""), None);
    }

    /// A push sender that records the tokens it was handed, for the relay test.
    #[derive(Default)]
    struct RecordingSender {
        pushed: Mutex<Vec<String>>,
    }
    impl PushSender for RecordingSender {
        fn push<'a>(
            &'a self,
            devices: &'a [DeviceToken],
            _title: &'a str,
            _body: &'a str,
            _link: Option<&'a str>,
        ) -> crate::push::PushFuture<'a> {
            let toks: Vec<String> = devices.iter().map(|d| d.token.clone()).collect();
            self.pushed.lock().unwrap().extend(toks.iter().cloned());
            Box::pin(async move { toks.len() })
        }
    }

    #[tokio::test]
    async fn notify_owner_fans_out_to_the_owners_devices() {
        let registry: Arc<dyn DeviceRegistry> = Arc::new(InMemoryDeviceRegistry::new());
        registry
            .register("owner", DeviceToken { token: "t1".into(), platform: "ios".into() })
            .await;
        let sender = Arc::new(RecordingSender::default());
        let state = RelayState::new(Arc::new(Relay::new()), Arc::new(DevTokenAuth))
            .with_push(registry, sender.clone());

        let link = "https://app.darkrun.ai/runs/r";
        assert_eq!(state.notify_owner("owner", "T", "B", Some(link)).await, 1);
        assert_eq!(*sender.pushed.lock().unwrap(), vec!["t1".to_string()]);
        // An owner with no devices pushes nothing.
        assert_eq!(state.notify_owner("ghost", "T", "B", Some(link)).await, 0);
    }

    // ── Descriptor API (GET /api/runs/{slug}/relay) ──────────────────────────

    #[tokio::test]
    async fn descriptor_returns_the_attach_for_the_owner_and_404s_for_everyone_else() {
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let relay = Arc::new(Relay::new());
        // acct-a hosts run "quiet-canyon" locally (owner fixed at registration).
        let _host_rx = relay.register_host("quiet-canyon", "acct-a").await.unwrap();
        let state = RelayState::new(relay, Arc::new(DevTokenAuth))
            // Trailing slash trimmed by with_relay_url.
            .with_relay_url("wss://relay.darkrun.ai/");
        let app = relay_descriptor_router(state);

        let get = |slug: &str, auth: Option<&str>| {
            let mut req = axum::http::Request::get(format!("/api/runs/{slug}/relay"));
            if let Some(a) = auth {
                req = req.header("authorization", format!("Bearer {a}"));
            }
            req.body(axum::body::Body::empty()).unwrap()
        };

        // The OWNER gets a 200 with the relay base, the session (== slug), and the
        // caller's own token echoed back as the client token.
        let res = app.clone().oneshot(get("quiet-canyon", Some("acct-a"))).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["relay_url"], "wss://relay.darkrun.ai");
        assert_eq!(v["session"], "quiet-canyon");
        assert_eq!(v["client_token"], "acct-a");

        // A DIFFERENT account (authenticated, but not the owner) → 404, never a
        // descriptor for a run it doesn't own.
        let res = app.clone().oneshot(get("quiet-canyon", Some("acct-b"))).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let _ = res.into_body().collect().await;

        // A run with no live host → 404.
        let res = app.clone().oneshot(get("ghost", Some("acct-a"))).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let _ = res.into_body().collect().await;

        // No bearer token → 401.
        let res = app.oneshot(get("quiet-canyon", None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn host_owned_by_reads_the_registry_owner_scoped_doc_when_shared() {
        // With a shared registry, reachability comes off the owner-scoped
        // `(owner, session)` doc — the same source the attach path authorizes on.
        let registry: Arc<dyn SessionRegistry> = Arc::new(FakeRegistry::default());
        let relay = Relay::new().with_registry(registry);
        let _host_rx = relay.register_host("s1", "acct-a").await.unwrap();

        assert!(relay.host_owned_by("s1", "acct-a").await, "the owner is reachable");
        assert!(
            !relay.host_owned_by("s1", "acct-b").await,
            "a different owner's doc is absent → not reachable"
        );
        assert!(!relay.host_owned_by("ghost", "acct-a").await, "no host → not reachable");
    }

    #[tokio::test]
    async fn device_endpoints_register_and_unregister_for_the_caller() {
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let registry: Arc<dyn DeviceRegistry> = Arc::new(InMemoryDeviceRegistry::new());
        let state = RelayState::new(Arc::new(Relay::new()), Arc::new(DevTokenAuth))
            .with_push(registry.clone(), Arc::new(NoopPushSender));
        let app = device_router(state);

        // No bearer token → 401, nothing registered.
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::post("/devices")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"token":"d1","platform":"ios"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert!(registry.devices_for("acct-a").await.is_empty());

        // With a bearer token (DevTokenAuth: token == account) → registered.
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::post("/devices")
                    .header("authorization", "Bearer acct-a")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"token":"d1","platform":"ios"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            registry.devices_for("acct-a").await,
            vec![DeviceToken { token: "d1".into(), platform: "ios".into() }]
        );

        // DELETE drops it (drains the body to satisfy the oneshot).
        let res = app
            .oneshot(
                axum::http::Request::delete("/devices/d1")
                    .header("authorization", "Bearer acct-a")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let _ = res.into_body().collect().await;
        assert!(registry.devices_for("acct-a").await.is_empty());
    }

    #[tokio::test]
    async fn delete_cannot_drop_another_accounts_device() {
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let registry: Arc<dyn DeviceRegistry> = Arc::new(InMemoryDeviceRegistry::new());
        // acct-a owns device "d1".
        registry
            .register("acct-a", DeviceToken { token: "d1".into(), platform: "ios".into() })
            .await;
        let state = RelayState::new(Arc::new(Relay::new()), Arc::new(DevTokenAuth))
            .with_push(registry.clone(), Arc::new(NoopPushSender));
        let app = device_router(state);

        // acct-b (authenticated, but not the owner) tries to delete acct-a's device.
        let res = app
            .oneshot(
                axum::http::Request::delete("/devices/d1")
                    .header("authorization", "Bearer acct-b")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Idempotent no-op (no existence oracle), but the device MUST survive.
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let _ = res.into_body().collect().await;
        assert_eq!(
            registry.devices_for("acct-a").await,
            vec![DeviceToken { token: "d1".into(), platform: "ios".into() }],
            "a non-owner must not be able to unregister the device"
        );
    }

    #[tokio::test]
    async fn devices_cors_preflight_allows_the_app_origin() {
        use tower::ServiceExt;

        let registry: Arc<dyn DeviceRegistry> = Arc::new(InMemoryDeviceRegistry::new());
        let state = RelayState::new(Arc::new(Relay::new()), Arc::new(DevTokenAuth))
            .with_push(registry, Arc::new(NoopPushSender));
        let app = device_router(state);

        // A browser preflight from app.darkrun.ai gets the allow-origin back.
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .method("OPTIONS")
                    .uri("/devices")
                    .header("origin", "https://app.darkrun.ai")
                    .header("access-control-request-method", "POST")
                    .header("access-control-request-headers", "authorization,content-type")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(res.status().is_success());
        assert_eq!(
            res.headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("https://app.darkrun.ai"),
        );
    }

    #[tokio::test]
    async fn host_registers_once_and_attach_signals_join() {
        let relay = Relay::new();
        assert!(!relay.has_host("s1"));

        let mut host_rx = relay
            .register_host("s1", "acct-a")
            .await
            .expect("first host registers");
        assert!(relay.has_host("s1"));
        assert!(
            relay.register_host("s1", "acct-a").await.is_none(),
            "one host per session"
        );

        // Owner match attaches AND the host is told a client joined.
        let (cid, _crx) = relay.attach_client("s1", "acct-a").await.expect("owner attaches");
        assert_eq!(host_rx.try_recv().unwrap(), HostEvent::Join { client: cid });
        assert_eq!(relay.client_count("s1"), 1);

        // Mismatched owner is forbidden; absent session is NoHost.
        assert!(matches!(
            relay.attach_client("s1", "acct-b").await,
            Err(AttachError::Forbidden)
        ));
        assert!(matches!(
            relay.attach_client("ghost", "acct-a").await,
            Err(AttachError::NoHost)
        ));

        // Detach signals leave.
        relay.detach_client("s1", cid).await;
        assert_eq!(host_rx.try_recv().unwrap(), HostEvent::Leave { client: cid });
        assert_eq!(relay.client_count("s1"), 0);
    }

    #[tokio::test]
    async fn client_frames_reach_the_host_tagged_with_the_client_id() {
        let relay = Relay::new();
        let mut host_rx = relay.register_host("s", "acct").await.unwrap();
        let (cid, _crx) = relay.attach_client("s", "acct").await.unwrap();
        assert_eq!(host_rx.try_recv().unwrap(), HostEvent::Join { client: cid });

        assert!(relay.from_client("s", cid, Frame::Text("answer".into())).await);
        assert_eq!(
            host_rx.try_recv().unwrap(),
            HostEvent::Msg { client: cid, data: "answer".into() }
        );

        // No host → nowhere to go.
        relay.drop_host("s").await;
        assert!(!relay.from_client("s", cid, Frame::Text("late".into())).await);
    }

    #[tokio::test]
    async fn host_routes_a_frame_to_one_client_only() {
        let relay = Relay::new();
        let _host_rx = relay.register_host("s", "acct").await.unwrap();
        let (c1, mut rx1) = relay.attach_client("s", "acct").await.unwrap();
        let (_c2, mut rx2) = relay.attach_client("s", "acct").await.unwrap();

        // A frame addressed to c1 reaches ONLY c1 — each client has its own
        // subscription (snapshot-on-connect), not a broadcast.
        relay.to_client("s", c1, Frame::Text("snapshot-for-1".into())).await;
        assert_eq!(rx1.try_recv().unwrap(), Frame::Text("snapshot-for-1".into()));
        assert!(rx2.try_recv().is_err());
    }

    #[tokio::test]
    async fn dropping_the_host_closes_client_receivers() {
        let relay = Relay::new();
        let _host_rx = relay.register_host("s", "acct").await.unwrap();
        let (_c, mut crx) = relay.attach_client("s", "acct").await.unwrap();

        relay.drop_host("s").await;
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

    // ── Shared registry path (offline fake — no Firestore) ───────────────────

    /// An in-memory [`SessionRegistry`] modelling single-instance semantics so the
    /// relay's shared-path branch (registry authz + single-host CAS) is exercised
    /// without a network. Keyed by `(owner, session) -> instance`, mirroring the
    /// owner-scoped Firestore doc: a different owner is a different entry, so
    /// cross-owner collision is structurally impossible.
    #[derive(Default)]
    struct FakeRegistry {
        sessions: Mutex<HashMap<(String, String), String>>,
    }

    impl SessionRegistry for FakeRegistry {
        fn register_host<'a>(
            &'a self,
            session: &'a str,
            owner: &'a str,
            instance: &'a str,
        ) -> crate::relay_registry::SessionRegistryFuture<'a, bool> {
            let won = {
                use std::collections::hash_map::Entry;
                let mut map = self.sessions.lock().unwrap();
                match map.entry((owner.to_string(), session.to_string())) {
                    Entry::Vacant(e) => {
                        e.insert(instance.to_string());
                        true
                    }
                    // A live host already holds this (owner, session).
                    Entry::Occupied(_) => false,
                }
            };
            Box::pin(async move { won })
        }

        fn heartbeat<'a>(
            &'a self,
            _session: &'a str,
            _owner: &'a str,
            _instance: &'a str,
        ) -> crate::relay_registry::SessionRegistryFuture<'a, ()> {
            Box::pin(async {})
        }

        fn drop_host<'a>(
            &'a self,
            session: &'a str,
            owner: &'a str,
            instance: &'a str,
        ) -> crate::relay_registry::SessionRegistryFuture<'a, ()> {
            {
                let mut map = self.sessions.lock().unwrap();
                let key = (owner.to_string(), session.to_string());
                if map.get(&key).map(String::as_str) == Some(instance) {
                    map.remove(&key);
                }
            }
            Box::pin(async {})
        }

        fn lookup_owner<'a>(
            &'a self,
            session: &'a str,
            owner: &'a str,
        ) -> crate::relay_registry::SessionRegistryFuture<'a, Option<String>> {
            let found = self
                .sessions
                .lock()
                .unwrap()
                .contains_key(&(owner.to_string(), session.to_string()))
                .then(|| owner.to_string());
            Box::pin(async move { found })
        }

        fn lookup_host_instance<'a>(
            &'a self,
            session: &'a str,
            owner: &'a str,
        ) -> crate::relay_registry::SessionRegistryFuture<'a, Option<String>> {
            let found = self
                .sessions
                .lock()
                .unwrap()
                .get(&(owner.to_string(), session.to_string()))
                .cloned();
            Box::pin(async move { found })
        }
    }

    #[tokio::test]
    async fn shared_registry_gates_second_host_and_sources_attach_authz() {
        let registry: Arc<dyn SessionRegistry> = Arc::new(FakeRegistry::default());
        let relay = Relay::new().with_registry(registry.clone());

        // First host wins the registry CAS; a second is refused (one host).
        let _host_rx = relay
            .register_host("s1", "acct-a")
            .await
            .expect("first host wins the registry claim");
        assert!(relay.has_host("s1"));
        assert!(
            relay.register_host("s1", "acct-a").await.is_none(),
            "a second host loses the registry claim"
        );

        // Attach authz is sourced from the registry's owner-scoped doc, not the
        // local owner field: the owner attaches, a DIFFERENT account resolves to a
        // different (absent) doc so it's NoHost (never a cross-owner Forbidden —
        // that case is structurally impossible now), and an unknown session is
        // NoHost.
        let (_cid, _crx) = relay
            .attach_client("s1", "acct-a")
            .await
            .expect("registry authorizes the owner");
        assert!(matches!(
            relay.attach_client("s1", "acct-b").await,
            Err(AttachError::NoHost)
        ));
        assert!(matches!(
            relay.attach_client("ghost", "acct-a").await,
            Err(AttachError::NoHost)
        ));

        // Dropping the host clears the registry so the session frees up.
        relay.drop_host("s1").await;
        assert!(!relay.has_host("s1"));
        assert!(
            relay.register_host("s1", "acct-c").await.is_some(),
            "a freed session can be re-registered"
        );
    }

    #[tokio::test]
    async fn shared_registry_authz_but_host_on_another_instance_is_nohost() {
        // The registry authorizes (the session exists + owner matches), but the
        // host isn't on THIS instance's local map — frames are in-memory this
        // landing, so attach is NoHost until the Step 1c cross-instance frame bus.
        let registry: Arc<dyn SessionRegistry> = Arc::new(FakeRegistry::default());
        // Register the session under a DIFFERENT instance id, directly in the fake.
        registry.register_host("s2", "acct-a", "other-instance").await;

        let relay = Relay::new().with_registry(registry);
        assert!(!relay.has_host("s2"), "host is not on this instance's local map");
        assert!(matches!(
            relay.attach_client("s2", "acct-a").await,
            Err(AttachError::NoHost),
        ));
    }

    #[tokio::test]
    async fn shared_registry_never_attaches_a_client_to_another_owners_local_host() {
        // Owner-scoping lets two accounts hold the SAME (low-entropy) session id in
        // separate docs. If acct-a hosts "s1" locally here while acct-b's "s1" doc
        // is live on another instance, a client for acct-b must NOT be routed to
        // acct-a's local host — its owner-scoped authz passes, but the local conn
        // belongs to a different owner, so attach is NoHost (its real host is
        // elsewhere), never a cross-owner leak.
        let registry: Arc<dyn SessionRegistry> = Arc::new(FakeRegistry::default());
        let relay = Relay::new().with_registry(registry.clone());

        // acct-a hosts "s1" locally on THIS instance.
        let mut host_rx = relay
            .register_host("s1", "acct-a")
            .await
            .expect("acct-a hosts s1 locally");
        // acct-b's "s1" doc is live too (a different doc), hosted on another instance.
        registry.register_host("s1", "acct-b", "other-instance").await;

        // A client for acct-b is authorized by its own doc but must land NoHost —
        // not on acct-a's conn.
        assert!(matches!(
            relay.attach_client("s1", "acct-b").await,
            Err(AttachError::NoHost),
        ));
        // acct-a's host was never signalled a join and has no clients attached.
        assert!(host_rx.try_recv().is_err(), "acct-a's host saw no cross-owner join");
        assert_eq!(relay.client_count("s1"), 0);

        // acct-a's own client still attaches to acct-a's local host.
        let (_cid, _crx) = relay
            .attach_client("s1", "acct-a")
            .await
            .expect("acct-a's own client attaches to acct-a's host");
        assert_eq!(relay.client_count("s1"), 1);
    }

    // ── Cross-instance frame bus (Step 1c) ───────────────────────────────────

    /// A [`FrameBus`] that records every publish, so the fast-path-vs-bus routing
    /// DECISION is asserted without a network.
    #[derive(Default)]
    struct RecordingBus {
        published: Mutex<Vec<(String, String, BusFrame)>>, // (to_instance, session, frame)
    }

    impl FrameBus for RecordingBus {
        fn publish<'a>(
            &'a self,
            to_instance: &'a str,
            session: &'a str,
            frame: BusFrame,
        ) -> crate::relay_bus::FrameBusFuture<'a, ()> {
            self.published
                .lock()
                .unwrap()
                .push((to_instance.to_string(), session.to_string(), frame));
            Box::pin(async {})
        }
    }

    #[tokio::test]
    async fn to_client_delivers_locally_for_a_local_client_and_publishes_for_a_remote_one() {
        let registry: Arc<dyn SessionRegistry> = Arc::new(FakeRegistry::default());
        let bus = Arc::new(RecordingBus::default());
        let relay = Relay::new().with_registry(registry).with_bus(bus.clone());

        // A host on THIS instance, with a local client attached here.
        let _host_rx = relay.register_host("s1", "acct").await.unwrap();
        let (local_id, mut local_rx) = relay.attach_client("s1", "acct").await.unwrap();

        // A client on ANOTHER instance joins (as the bus subscriber would deliver),
        // populating the host's client→instance routing table.
        let remote_id = 4242;
        relay.deliver_bus_frame(
            "s1",
            BusFrame::to_host("acct", "inst-b", HostEvent::Join { client: remote_id }),
        );

        // A frame to the LOCAL client is delivered locally — 0 bus hops.
        relay.to_client("s1", local_id, Frame::Text("for-local".into())).await;
        assert_eq!(local_rx.try_recv().unwrap(), Frame::Text("for-local".into()));
        assert!(bus.published.lock().unwrap().is_empty(), "local delivery publishes nothing");

        // A frame to the REMOTE client is published to that client's instance.
        relay.to_client("s1", remote_id, Frame::Text("for-remote".into())).await;
        let published = bus.published.lock().unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, "inst-b");
        assert_eq!(published[0].1, "s1");
        assert_eq!(
            published[0].2,
            BusFrame::to_client("acct", remote_id, "for-remote")
        );
    }

    #[tokio::test]
    async fn attach_to_a_remote_host_registers_locally_and_bridges_over_the_bus() {
        let registry: Arc<dyn SessionRegistry> = Arc::new(FakeRegistry::default());
        let bus = Arc::new(RecordingBus::default());
        // The host lives on ANOTHER instance (registered directly in the fake).
        registry.register_host("s9", "acct", "inst-a").await;
        let relay = Relay::new().with_registry(registry).with_bus(bus.clone());
        let this_instance = relay.instance_id().to_string();

        // No local host here, but the bus lets the client attach: its sink lives
        // here and a Join is published to the host's instance.
        let (id, mut rx) = relay
            .attach_client("s9", "acct")
            .await
            .expect("attaches to a remote host via the bus");
        assert_eq!(
            *bus.published.lock().unwrap(),
            vec![(
                "inst-a".to_string(),
                "s9".to_string(),
                BusFrame::to_host("acct", &this_instance, HostEvent::Join { client: id }),
            )]
        );

        // A frame the remote host addressed to this client arrives over the bus and
        // is delivered into the client's local sink.
        relay.deliver_bus_frame("s9", BusFrame::to_client("acct", id, "snapshot"));
        assert_eq!(rx.try_recv().unwrap(), Frame::Text("snapshot".into()));

        // A frame the client sends is published to the host's instance as a Msg.
        assert!(relay.from_client("s9", id, Frame::Text("advance".into())).await);
        assert_eq!(
            bus.published.lock().unwrap()[1],
            (
                "inst-a".to_string(),
                "s9".to_string(),
                BusFrame::to_host(
                    "acct",
                    &this_instance,
                    HostEvent::Msg { client: id, data: "advance".into() },
                ),
            )
        );

        // Detach publishes Leave to the host's instance, and the client is gone.
        relay.detach_client("s9", id).await;
        assert_eq!(
            bus.published.lock().unwrap()[2].2,
            BusFrame::to_host("acct", &this_instance, HostEvent::Leave { client: id })
        );
        relay.deliver_bus_frame("s9", BusFrame::to_client("acct", id, "late"));
        assert!(rx.try_recv().is_err(), "a detached remote client receives nothing");
    }

    #[tokio::test]
    async fn deliver_bus_frame_drops_a_cross_owner_to_host_event() {
        // Owner-scoping lets two accounts hold the same low-entropy slug. A host on
        // THIS instance for acct-a must NEVER receive (nor route) a bus frame tagged
        // for acct-b, even though both name session "s1".
        let registry: Arc<dyn SessionRegistry> = Arc::new(FakeRegistry::default());
        let bus = Arc::new(RecordingBus::default());
        let relay = Relay::new().with_registry(registry).with_bus(bus);
        let mut host_rx = relay.register_host("s1", "acct-a").await.unwrap();

        // A cross-owner Join is dropped at delivery: the host is never signalled,
        // and no client→instance routing-table entry is recorded.
        relay.deliver_bus_frame(
            "s1",
            BusFrame::to_host("acct-b", "inst-b", HostEvent::Join { client: 77 }),
        );
        assert!(
            host_rx.try_recv().is_err(),
            "a frame for another owner must not reach acct-a's host"
        );

        // A same-owner Join is delivered, and a subsequent to_client for that
        // client routes over the bus — proving the drop was owner-specific, not a
        // routing-table miss.
        relay.deliver_bus_frame(
            "s1",
            BusFrame::to_host("acct-a", "inst-b", HostEvent::Join { client: 42 }),
        );
        assert_eq!(host_rx.try_recv().unwrap(), HostEvent::Join { client: 42 });
    }

    #[tokio::test]
    async fn deliver_bus_frame_drops_a_cross_owner_to_client_frame() {
        // A remote client for acct-a attached HERE (its host is on another
        // instance). A ToClient bus frame tagged acct-b must not be delivered into
        // acct-a's client sink — the owner gate drops it and never falls through.
        let registry: Arc<dyn SessionRegistry> = Arc::new(FakeRegistry::default());
        let bus = Arc::new(RecordingBus::default());
        registry.register_host("s9", "acct-a", "inst-a").await;
        let relay = Relay::new().with_registry(registry).with_bus(bus);

        let (id, mut rx) = relay
            .attach_client("s9", "acct-a")
            .await
            .expect("acct-a's client attaches to its remote host");

        // Cross-owner frame → dropped.
        relay.deliver_bus_frame("s9", BusFrame::to_client("acct-b", id, "leak"));
        assert!(
            rx.try_recv().is_err(),
            "a frame for another owner must not reach acct-a's client"
        );

        // Same-owner frame → delivered.
        relay.deliver_bus_frame("s9", BusFrame::to_client("acct-a", id, "ok"));
        assert_eq!(rx.try_recv().unwrap(), Frame::Text("ok".into()));
    }

    #[tokio::test]
    async fn without_a_bus_a_remote_host_is_nohost_exactly_as_before() {
        // Registry authorizes (session live under another instance), but with NO
        // bus wired a split pair can't exchange frames — attach is NoHost, the
        // single-instance behavior from before Step 1c.
        let registry: Arc<dyn SessionRegistry> = Arc::new(FakeRegistry::default());
        registry.register_host("s", "acct", "other-instance").await;
        let relay = Relay::new().with_registry(registry); // no .with_bus(...)
        assert!(matches!(
            relay.attach_client("s", "acct").await,
            Err(AttachError::NoHost)
        ));
    }
}
