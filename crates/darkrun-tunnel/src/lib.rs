//! Host connector — durably bridges the **relay** to the engine's **local
//! HTTP/WS** server, so a remote client reaches a live run exactly as the desktop
//! does on loopback.
//!
//! The host dials OUTBOUND to the relay's host endpoint and parks a WebSocket
//! (NAT-proof). The relay then drives it with [`HostEvent`]s:
//!
//! - **`Join{client}`** → open that client its OWN local `/ws/session/{run}`
//!   subscription. The local server pushes a snapshot first, so the client reads
//!   into the live session on connect; each local push is wrapped as a
//!   [`ServerFrame`] (first → `Snapshot`, then `Update`) and routed back with
//!   [`HostCmd::To`].
//! - **`Leave{client}`** → tear that client's subscription down.
//! - **`Msg{client,data}`** → a [`ClientFrame`]: `Ping` → `Pong`; `Cmd` →
//!   translate the [`ClientCommand`] into the engine's local REST write and
//!   `Ack` it.
//!
//! Durability: the relay connection runs under [`run`], which reconnects with a
//! fixed backoff after any drop; a periodic WS ping detects a dead peer. On
//! reconnect every client re-attaches and gets a fresh snapshot, so no state is
//! lost. Command acks + client-side retry make writes exactly-once in effect: the
//! host de-dupes on `(instance, id)` — the client's STABLE page-load instance,
//! not the relay's per-socket connection id — so a resend on a fresh socket after
//! a reconnect collapses to one exec.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use darkrun_api::notify::gate_message;
use darkrun_api::session::SessionPayload;
use darkrun_api::tunnel::{ClientCommand, ClientFrame, HostCmd, HostEvent, Seq, ServerFrame};
use futures_util::{SinkExt, StreamExt};
use reqwest::Method;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

/// How the connector reaches the relay and the local engine server.
#[derive(Debug, Clone)]
pub struct ConnectorConfig {
    /// The fully-formed relay host URL, incl. the session + token query
    /// (`wss://relay/relay/host/{session}?token=...`).
    pub relay_host_url: String,
    /// The local engine HTTP base (`http://127.0.0.1:{port}`); the WS base is
    /// derived by swapping the scheme.
    pub local_http_base: String,
    /// The run slug each remote client subscribes to.
    pub run: String,
    /// Backoff between relay reconnect attempts.
    pub reconnect: Duration,
}

impl ConnectorConfig {
    /// The local WS subscription URL for this run (`ws://…/ws/session/{run}`).
    fn local_ws_url(&self) -> String {
        let ws_base = self
            .local_http_base
            .replacen("http://", "ws://", 1)
            .replacen("https://", "wss://", 1);
        format!("{ws_base}/ws/session/{}", self.run)
    }
}

/// A local HTTP write the connector issues on the engine's loopback server.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalRequest {
    method: Method,
    path: String,
    body: Option<Value>,
}

/// Map a [`ClientCommand`] to the engine's local REST write — the tunnel
/// equivalent of the desktop's REST calls. Pure, so it's unit-tested directly.
fn command_request(cmd: &ClientCommand) -> LocalRequest {
    match cmd {
        ClientCommand::Advance { run } => LocalRequest {
            method: Method::POST,
            path: format!("/api/advance/{run}"),
            body: None,
        },
        ClientCommand::Answer { session, answer } => LocalRequest {
            method: Method::POST,
            path: format!("/question/{session}/answer"),
            body: Some(answer.clone()),
        },
        ClientCommand::Feedback { run, station, body } => LocalRequest {
            method: Method::POST,
            path: format!("/api/feedback/{run}/{station}"),
            body: Some(serde_json::json!({ "body": body })),
        },
        // A checkpoint decision — the `ReviewDecisionRequest` shape the
        // `/review/:id/decide` endpoint expects. An approve omits the feedback
        // field; request-changes carries the operator's note.
        ClientCommand::Decide { session, decision, note } => {
            let mut body = serde_json::json!({ "decision": decision });
            if let Some(note) = note {
                body["feedback"] = Value::String(note.clone());
            }
            LocalRequest {
                method: Method::POST,
                path: format!("/review/{session}/decide"),
                body: Some(body),
            }
        }
        // A design-direction choice routes to the direction-select endpoint (the
        // `DirectionSelectRequest` body), NOT the question one.
        ClientCommand::Direction { session, archetype } => LocalRequest {
            method: Method::POST,
            path: format!("/direction/{session}/select"),
            body: Some(serde_json::json!({ "archetype": archetype })),
        },
        // A picker selection routes to the picker-select endpoint (the
        // `PickerSelectRequest` body).
        ClientCommand::Picker { session, option } => LocalRequest {
            method: Method::POST,
            path: format!("/picker/{session}/select"),
            body: Some(serde_json::json!({ "id": option })),
        },
    }
}

/// Wrap a raw local push (a serialized review payload) into a [`ServerFrame`].
/// The first push of a (re)connection is the snapshot; the rest are updates.
/// Pure, so it's unit-tested directly.
fn wrap_payload(seq: Seq, raw: &str, first: bool) -> ServerFrame {
    let payload = serde_json::from_str::<Value>(raw).unwrap_or(Value::Null);
    if first {
        ServerFrame::Snapshot { seq, payload }
    } else {
        ServerFrame::Update { seq, payload }
    }
}

/// Serialize a [`ServerFrame`] addressed to `client` into the relay envelope
/// message, queued on `out`.
fn route_to_client(client: u64, frame: &ServerFrame, out: &mpsc::UnboundedSender<Message>) {
    if let Ok(data) = serde_json::to_string(frame) {
        if let Ok(json) = serde_json::to_string(&HostCmd::To { client, data }) {
            let _ = out.send(Message::Text(json.into()));
        }
    }
}

/// Queue a [`HostCmd::Notify`] for the relay to fan out to the owner's remote
/// devices — the remote half of "notify as the engine ticks".
fn route_notify(title: &str, body: &str, out: &mpsc::UnboundedSender<Message>) {
    let cmd = HostCmd::Notify { title: title.to_string(), body: body.to_string() };
    if let Ok(json) = serde_json::to_string(&cmd) {
        let _ = out.send(Message::Text(json.into()));
    }
}

/// The `(run, station)` a local session payload is parked at an operator gate
/// on, or `None` when it isn't. A `Review` payload with a `gate_type` set is "at
/// a gate"; the station is the one that entered its gate without an outcome yet
/// (empty if none is pinpointable). Pure, so the gate signal is unit-tested.
fn gate_target(raw: &str) -> Option<(String, String)> {
    let SessionPayload::Review(review) = serde_json::from_str::<SessionPayload>(raw).ok()? else {
        return None;
    };
    review.gate_type.as_ref()?; // not parked at a gate
    let run = review.run_slug.unwrap_or_default();
    let station = review
        .station_states
        .iter()
        .find(|s| s.gate_entered_at.is_some() && s.gate_outcome.is_none())
        .map(|s| s.station.clone())
        .unwrap_or_default();
    Some((run, station))
}

/// Edge-detects gate ENTRY across a stream of local payloads, so the connector
/// pushes once when a gate opens — not on every update while parked at it, and
/// not on a reconnect that merely observes an already-open gate.
#[derive(Default)]
struct GateWatcher {
    /// The gate target last seen (`None` = not at a gate).
    last: Option<(String, String)>,
    /// Whether the first payload has been observed. The first is a SILENT
    /// baseline: a fresh (re)connection that finds a gate already open does not
    /// re-notify — only a transition INTO a gate during the session does.
    primed: bool,
}

impl GateWatcher {
    /// Feed one local payload; return the `(title, body)` to push if it's a
    /// fresh gate entry.
    fn observe(&mut self, raw: &str) -> Option<(String, String)> {
        let target = gate_target(raw);
        let message = if self.primed && target.is_some() && target != self.last {
            target
                .as_ref()
                .map(|(run, station)| gate_message(run, station))
        } else {
            None
        };
        self.primed = true;
        self.last = target;
        message
    }
}

/// Run the host connector: connect to the relay, serve until the connection
/// drops, then reconnect after the configured backoff. Loops on TRANSIENT drops
/// (DNS/TCP/TLS/5xx/protocol) forever, so it returns only when either the caller
/// cancels the task OR the relay REJECTS the dial credential.
///
/// An auth rejection (the relay answers the WS handshake with 401/403) means the
/// baked dial token is stale — retrying it in place would 401 forever, which is
/// exactly the long-run failure crit#6 fixes. So `run` EXITS on an auth
/// rejection, surfacing it to the dial supervisor (which re-resolves + refreshes
/// the credential and re-dials with a fresh token) instead of silently hammering
/// the relay with a dead token.
pub async fn run(cfg: ConnectorConfig) {
    let http = reqwest::Client::new();
    // One command-dedup cache for the whole SESSION, hoisted above the reconnect
    // loop. A gate command can be redelivered across a reconnect (both the bus's
    // at-least-once window and a reconnect are seconds-wide); a per-connection
    // cache would be empty after the reconnect and re-run the command, so this
    // must survive reconnects to never double-submit advance/answer/feedback. Its
    // TTL still bounds it.
    let dedup = Arc::new(Mutex::new(CommandDedup::new()));
    loop {
        match serve_once(&cfg, &http, &dedup).await {
            Ok(()) => {}
            Err(e) if e.auth => {
                tracing::error!(
                    "darkrun-tunnel: {} — the /darkrun:darkrun-login credential is stale; \
                     a refresh + re-dial is needed (not retrying this token in place)",
                    e.message
                );
                return;
            }
            Err(e) => {
                tracing::warn!("darkrun-tunnel: relay connection ended: {}", e.message);
            }
        }
        tokio::time::sleep(cfg.reconnect).await;
    }
}

/// Why a relay connection attempt ended: an AUTH rejection (the relay answered
/// the WS handshake with 401/403, so the dial token is stale and a fresh one is
/// needed) versus a transient dial/stream failure worth retrying in place.
#[derive(Debug)]
struct DialError {
    /// The relay rejected the dial credential at the WS handshake (401/403).
    auth: bool,
    /// A human-readable reason, for the log line.
    message: String,
}

impl DialError {
    /// A retryable (non-auth) failure.
    fn transient(message: String) -> Self {
        Self { auth: false, message }
    }
}

/// Whether an HTTP status at the WS handshake is an AUTH rejection (the relay
/// refused the dial token) rather than a transient failure.
fn is_auth_status(status: u16) -> bool {
    status == 401 || status == 403
}

/// Classify a WS-handshake failure. A tungstenite `Http` response carrying a
/// 401/403 is an auth rejection — the baked dial token is stale, so the caller
/// must re-dial with a FRESH one rather than retry this token; everything else
/// (DNS, TCP, TLS, a 5xx, a protocol error) is transient and retried in place.
fn classify_dial_error(err: &tokio_tungstenite::tungstenite::Error) -> DialError {
    if let tokio_tungstenite::tungstenite::Error::Http(resp) = err {
        let status = resp.status().as_u16();
        if is_auth_status(status) {
            return DialError {
                auth: true,
                message: format!("relay rejected the dial token (HTTP {status})"),
            };
        }
        return DialError::transient(format!("relay handshake failed (HTTP {status})"));
    }
    DialError::transient(format!("dialing relay: {err}"))
}

/// One relay connection's lifetime: register, then bridge clients until the
/// socket closes or errors.
async fn serve_once(
    cfg: &ConnectorConfig,
    http: &reqwest::Client,
    dedup: &Arc<Mutex<CommandDedup>>,
) -> Result<(), DialError> {
    let (ws, _) = connect_async(&cfg.relay_host_url)
        .await
        .map_err(|e| classify_dial_error(&e))?;
    let (mut sink, mut stream) = ws.split();

    // One writer drains the outbound queue to the relay sink, so every task
    // (client subscriptions + command acks + the pinger) shares one sink safely.
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Heartbeat: a periodic WS ping surfaces a dead relay fast (the read loop
    // then errors and we reconnect).
    let ping_tx = out_tx.clone();
    let pinger = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(20));
        tick.tick().await; // the first tick is immediate
        loop {
            tick.tick().await;
            if ping_tx.send(Message::Ping(Vec::new().into())).is_err() {
                break;
            }
        }
    });

    // A single monitor subscription edge-detects gate entry and pushes a
    // notification — independent of clients, so a push fires even with nobody
    // attached (the operator is away; that's the point).
    let monitor = spawn_gate_monitor(cfg.local_ws_url(), out_tx.clone());

    // The command-dedup cache (`(instance, id)` -> state) is owned by `run` and
    // shared across reconnects, so an at-least-once bus redelivery — OR a
    // client's cross-reconnect resend under a fresh relay connection id — never
    // re-runs a non-idempotent command even when it straddles a reconnect.

    let mut clients: HashMap<u64, JoinHandle<()>> = HashMap::new();
    // A drop mid-stream is always transient — the handshake already succeeded, so
    // the token was accepted; only a fresh handshake can be an auth rejection.
    let result = read_loop(cfg, http, &mut stream, &out_tx, &mut clients, dedup)
        .await
        .map_err(DialError::transient);

    // Tear everything down — clients re-attach (with fresh snapshots) on reconnect.
    for (_, handle) in clients {
        handle.abort();
    }
    monitor.abort();
    writer.abort();
    pinger.abort();
    result
}

/// Open a dedicated monitor subscription to the local session and push a
/// [`HostCmd::Notify`] whenever the run ENTERS an operator gate. Runs for the
/// relay connection's lifetime, independent of any client subscriptions.
fn spawn_gate_monitor(local_ws_url: String, out: mpsc::UnboundedSender<Message>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let Ok((ws, _)) = connect_async(&local_ws_url).await else {
            return;
        };
        let (_sink, mut stream) = ws.split();
        let mut watcher = GateWatcher::default();
        while let Some(Ok(msg)) = stream.next().await {
            if let Message::Text(t) = msg {
                if let Some((title, body)) = watcher.observe(&t) {
                    route_notify(&title, &body, &out);
                }
            }
        }
    })
}

/// Drive the relay event stream until it ends.
async fn read_loop(
    cfg: &ConnectorConfig,
    http: &reqwest::Client,
    stream: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
              + Unpin),
    out_tx: &mpsc::UnboundedSender<Message>,
    clients: &mut HashMap<u64, JoinHandle<()>>,
    dedup: &Arc<Mutex<CommandDedup>>,
) -> Result<(), String> {
    while let Some(msg) = stream.next().await {
        let msg = msg.map_err(|e| format!("relay read: {e}"))?;
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Ping(p) => {
                let _ = out_tx.send(Message::Pong(p));
                continue;
            }
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(event) = serde_json::from_str::<HostEvent>(&text) else {
            continue;
        };
        match event {
            HostEvent::Join { client } => {
                let handle =
                    spawn_client_subscription(client, cfg.local_ws_url(), out_tx.clone());
                if let Some(old) = clients.insert(client, handle) {
                    old.abort();
                }
            }
            HostEvent::Leave { client } => {
                if let Some(handle) = clients.remove(&client) {
                    handle.abort();
                }
            }
            HostEvent::Msg { client, data } => {
                handle_client_frame(
                    client,
                    &data,
                    &cfg.local_http_base,
                    http.clone(),
                    out_tx.clone(),
                    Arc::clone(dedup),
                );
            }
        }
    }
    Ok(())
}

/// Open a client's local session subscription and forward each push back to it,
/// wrapped as a [`ServerFrame`]. The local server's first push is the snapshot.
fn spawn_client_subscription(
    client: u64,
    local_ws_url: String,
    out: mpsc::UnboundedSender<Message>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let Ok((ws, _)) = connect_async(&local_ws_url).await else {
            return;
        };
        let (_sink, mut stream) = ws.split();
        let mut seq: Seq = 0;
        let mut first = true;
        while let Some(Ok(msg)) = stream.next().await {
            if let Message::Text(t) = msg {
                let frame = wrap_payload(seq, &t, first);
                first = false;
                seq = seq.saturating_add(1);
                route_to_client(client, &frame, &out);
            }
        }
    })
}

/// The dedup key for a command: `(instance, id)`. The `instance` is the client's
/// STABLE per-page-load id (one browser tab / app launch), carried in each
/// [`ClientFrame::Cmd`] — NOT the relay's per-socket connection id, which is minted
/// fresh on every (re)connect. Keying on the instance means a client's resend on a
/// FRESH relay connection after a drop collapses to the same key (so a
/// non-idempotent write applies once), while two tabs — each numbering its `id`
/// from zero — get distinct instances and never collide.
type CmdKey = (String, String);

/// Cap on retained command-dedup entries. Commands are human-paced gate
/// decisions, so a few thousand is far above any real session's volume; the cap
/// only bounds memory against a misbehaving or abusive peer.
const DEDUP_CAPACITY: usize = 4096;

/// How long a completed command's ack stays cached for replay. A genuine bus
/// redelivery arrives within seconds, so evicting entries older than this only
/// ever drops replays far outside the redelivery window — never a correctness
/// loss for a real duplicate.
const DEDUP_TTL: Duration = Duration::from_secs(300);

/// A command's lifecycle in the per-connector dedup cache.
enum CmdState {
    /// `exec_command` is running for this `(instance, id)`; a duplicate must NOT
    /// start a second one. The in-flight exec caches its ack on completion, which
    /// a later retry replays to the client's current socket.
    InFlight,
    /// Completed — the cached ack to replay to a redelivered duplicate.
    Done(ServerFrame),
}

/// A dedup-cache entry: its state plus when it was first seen (for TTL pruning).
struct CmdEntry {
    state: CmdState,
    inserted: Instant,
}

/// What to do with an incoming `Cmd`, decided atomically under the dedup lock.
enum CmdAction {
    /// First sighting of this key — reserved InFlight; run `exec_command`.
    Execute,
    /// Already completed — replay the cached ack; do NOT execute.
    Replay(ServerFrame),
    /// A duplicate still InFlight — drop it. The original exec is running and
    /// caches its ack on completion; the client's next retry then hits the
    /// `Replay` path and gets that ack on its CURRENT socket. (Across a reconnect
    /// the original was dialed on a now-dead socket, so its direct ack may not
    /// land — the retry + replay recovers it, and the write still ran once.)
    Drop,
}

/// Per-connector command DEDUP.
///
/// A duplicate `ClientFrame::Cmd { instance, id, command }` can reach this host
/// connector MORE THAN ONCE — from the relay's at-least-once Pub/Sub bus, OR from
/// the client resending its pending set on a fresh socket after a reconnect.
/// advance/answer/feedback are NOT idempotent, so the invariant is: for each
/// `(instance, id)` key, `exec_command` runs **at most once**; every duplicate is
/// either dropped (the original is still in flight) or answered from the cached
/// ack. Keying on the client's STABLE `instance` (not the relay's per-socket
/// connection id, which is fresh on every reconnect) is what makes the
/// cross-reconnect resend collapse rather than double-apply. Bounded by capacity +
/// TTL so a long or abusive session can't grow it without limit; eviction only
/// ever affects replays older than the (seconds-wide) redelivery window.
struct CommandDedup {
    entries: HashMap<CmdKey, CmdEntry>,
    /// Keys in first-seen order — the front is the oldest, popped on eviction /
    /// TTL prune. Insertion order equals time order, so it doubles as the TTL
    /// queue.
    order: VecDeque<CmdKey>,
    capacity: usize,
    ttl: Duration,
}

impl CommandDedup {
    fn new() -> Self {
        Self::with_limits(DEDUP_CAPACITY, DEDUP_TTL)
    }

    fn with_limits(capacity: usize, ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
            ttl,
        }
    }

    /// Drop entries first seen more than `ttl` ago. The `order` queue is in
    /// first-seen (time) order, so expired entries are always at the front.
    fn prune(&mut self, now: Instant) {
        while let Some(front) = self.order.front() {
            let expired = self
                .entries
                .get(front)
                .is_none_or(|e| now.duration_since(e.inserted) > self.ttl);
            if !expired {
                break;
            }
            if let Some(key) = self.order.pop_front() {
                self.entries.remove(&key);
            }
        }
    }

    /// Evict the oldest entries until the cache is within `capacity`.
    fn evict_over_capacity(&mut self) {
        while self.entries.len() > self.capacity {
            let Some(key) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&key);
        }
    }

    /// Decide what to do with an incoming command for `key`, reserving the key
    /// InFlight on its first sighting.
    fn begin(&mut self, key: CmdKey, now: Instant) -> CmdAction {
        self.prune(now);
        match self.entries.get(&key) {
            Some(CmdEntry { state: CmdState::Done(ack), .. }) => CmdAction::Replay(ack.clone()),
            Some(CmdEntry { state: CmdState::InFlight, .. }) => CmdAction::Drop,
            None => {
                self.entries
                    .insert(key.clone(), CmdEntry { state: CmdState::InFlight, inserted: now });
                self.order.push_back(key);
                self.evict_over_capacity();
                CmdAction::Execute
            }
        }
    }

    /// Record a command's completed ack so a later duplicate replays it instead
    /// of re-executing.
    fn complete(&mut self, key: CmdKey, ack: ServerFrame, now: Instant) {
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.state = CmdState::Done(ack);
        } else {
            // Evicted while in flight (only under extreme volume) — re-cache so a
            // late duplicate still replays rather than re-executing.
            self.entries
                .insert(key.clone(), CmdEntry { state: CmdState::Done(ack), inserted: now });
            self.order.push_back(key);
            self.evict_over_capacity();
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Handle one [`ClientFrame`] from a client: heartbeat or a write command.
fn handle_client_frame(
    client: u64,
    data: &str,
    local_http_base: &str,
    http: reqwest::Client,
    out: mpsc::UnboundedSender<Message>,
    dedup: Arc<Mutex<CommandDedup>>,
) {
    let Ok(frame) = serde_json::from_str::<ClientFrame>(data) else {
        return;
    };
    match frame {
        ClientFrame::Ping => route_to_client(client, &ServerFrame::Pong, &out),
        // The subscription opened on Join already streams the snapshot.
        ClientFrame::Hello { .. } => {}
        ClientFrame::Cmd { instance, id, command } => {
            // Host-side command DEDUP, keyed on the client's STABLE page-load
            // `instance` + its `id` — NOT the relay's per-socket `client`, which is
            // minted fresh on every reconnect. This `Cmd` may be a redelivered
            // duplicate: from the at-least-once relay bus, OR from the client
            // resending its pending set on a FRESH socket after a drop (a new
            // `client`, same `instance`). INVARIANT: for each (instance, id) key,
            // `exec_command` runs AT MOST ONCE — a repeat that already completed
            // replays the CACHED ack (to this delivery's `client`), and a repeat
            // still in flight is dropped. This protects advance/answer/feedback,
            // which are not idempotent, from a double submit across a reconnect.
            let key: CmdKey = (instance, id.clone());
            let action = dedup.lock().unwrap().begin(key.clone(), Instant::now());
            match action {
                CmdAction::Replay(ack) => route_to_client(client, &ack, &out),
                CmdAction::Drop => {}
                CmdAction::Execute => {
                    let base = local_http_base.to_string();
                    let dedup = Arc::clone(&dedup);
                    tokio::spawn(async move {
                        let ack = match exec_command(&http, &base, &command).await {
                            Ok(()) => ServerFrame::Ack { id, ok: true, error: None },
                            Err(e) => ServerFrame::Ack { id, ok: false, error: Some(e) },
                        };
                        dedup.lock().unwrap().complete(key, ack.clone(), Instant::now());
                        route_to_client(client, &ack, &out);
                    });
                }
            }
        }
    }
}

/// Issue a command's local REST write against the engine's loopback server.
async fn exec_command(
    http: &reqwest::Client,
    base: &str,
    command: &ClientCommand,
) -> Result<(), String> {
    let req = command_request(command);
    let url = format!("{base}{}", req.path);
    let mut builder = http.request(req.method, &url);
    if let Some(body) = req.body {
        builder = builder.json(&body);
    }
    let resp = builder.send().await.map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("local server returned {}", resp.status()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_ws_url_swaps_scheme_and_targets_the_run() {
        let cfg = ConnectorConfig {
            relay_host_url: "wss://relay/relay/host/s?token=t".into(),
            local_http_base: "http://127.0.0.1:4317".into(),
            run: "quiet-canyon".into(),
            reconnect: Duration::from_secs(1),
        };
        assert_eq!(cfg.local_ws_url(), "ws://127.0.0.1:4317/ws/session/quiet-canyon");
    }

    #[test]
    fn auth_statuses_are_401_and_403_only() {
        assert!(is_auth_status(401));
        assert!(is_auth_status(403));
        // Everything else is transient — including neighbors and 5xx.
        for s in [101u16, 200, 400, 404, 429, 500, 502, 503] {
            assert!(!is_auth_status(s), "status {s} must not classify as auth");
        }
    }

    #[test]
    fn classify_dial_error_flags_401_403_as_auth_and_the_rest_transient() {
        use tokio_tungstenite::tungstenite::http::Response;
        use tokio_tungstenite::tungstenite::Error as WsError;

        let http_err = |status: u16| {
            let resp = Response::builder().status(status).body(None).unwrap();
            WsError::Http(Box::new(resp))
        };

        // A 401/403 handshake response is an auth rejection (re-dial needed).
        let e = classify_dial_error(&http_err(401));
        assert!(e.auth, "401 must be an auth rejection");
        assert!(e.message.contains("401"));
        assert!(classify_dial_error(&http_err(403)).auth);

        // A 5xx (or any other) handshake response is transient (retry in place).
        let e = classify_dial_error(&http_err(503));
        assert!(!e.auth, "503 is transient, not auth");
        assert!(e.message.contains("503"));

        // A non-HTTP failure (e.g. the socket closed) is transient too.
        let e = classify_dial_error(&WsError::ConnectionClosed);
        assert!(!e.auth, "a connection-closed error is transient");
    }

    #[test]
    fn command_request_maps_each_command_to_its_local_write() {
        let adv = command_request(&ClientCommand::Advance { run: "r".into() });
        assert_eq!(adv.method, Method::POST);
        assert_eq!(adv.path, "/api/advance/r");
        assert_eq!(adv.body, None);

        let ans = command_request(&ClientCommand::Answer {
            session: "sess".into(),
            answer: serde_json::json!({"choice": "a"}),
        });
        assert_eq!(ans.path, "/question/sess/answer");
        assert_eq!(ans.body, Some(serde_json::json!({"choice": "a"})));

        let fb = command_request(&ClientCommand::Feedback {
            run: "r".into(),
            station: "build".into(),
            body: "fix".into(),
        });
        assert_eq!(fb.path, "/api/feedback/r/build");
        assert_eq!(fb.body, Some(serde_json::json!({"body": "fix"})));
    }

    #[test]
    fn command_request_maps_decide_to_the_review_decide_endpoint() {
        // Approve → decide with no feedback field.
        let approve = command_request(&ClientCommand::Decide {
            session: "sess".into(),
            decision: "approved".into(),
            note: None,
        });
        assert_eq!(approve.method, Method::POST);
        assert_eq!(approve.path, "/review/sess/decide");
        assert_eq!(approve.body, Some(serde_json::json!({"decision": "approved"})));

        // Request-changes → decide with the reviewer note in `feedback`, the
        // exact shape `ReviewDecisionRequest` deserializes.
        let changes = command_request(&ClientCommand::Decide {
            session: "sess".into(),
            decision: "changes_requested".into(),
            note: Some("tighten the copy".into()),
        });
        assert_eq!(changes.path, "/review/sess/decide");
        assert_eq!(
            changes.body,
            Some(serde_json::json!({
                "decision": "changes_requested",
                "feedback": "tighten the copy",
            }))
        );
        // The body round-trips into the real request type the handler expects.
        let parsed: darkrun_api::ReviewDecisionRequest =
            serde_json::from_value(changes.body.unwrap()).unwrap();
        assert_eq!(parsed.decision, "changes_requested");
        assert_eq!(parsed.feedback.as_deref(), Some("tighten the copy"));
    }

    #[test]
    fn command_request_maps_direction_and_picker_to_their_select_endpoints() {
        let dir = command_request(&ClientCommand::Direction {
            session: "d1".into(),
            archetype: "bold".into(),
        });
        assert_eq!(dir.method, Method::POST);
        assert_eq!(dir.path, "/direction/d1/select");
        assert_eq!(dir.body, Some(serde_json::json!({"archetype": "bold"})));
        // The body matches the `DirectionSelectRequest` shape.
        let parsed: darkrun_api::DirectionSelectRequest =
            serde_json::from_value(dir.body.unwrap()).unwrap();
        assert_eq!(parsed.archetype, "bold");

        let pick = command_request(&ClientCommand::Picker {
            session: "p1".into(),
            option: "quick".into(),
        });
        assert_eq!(pick.method, Method::POST);
        assert_eq!(pick.path, "/picker/p1/select");
        assert_eq!(pick.body, Some(serde_json::json!({"id": "quick"})));
        // The body matches the `PickerSelectRequest` shape.
        let parsed: darkrun_api::PickerSelectRequest =
            serde_json::from_value(pick.body.unwrap()).unwrap();
        assert_eq!(parsed.id, "quick");
    }

    use darkrun_api::common::GateType;
    use darkrun_api::session::{ReviewSessionPayload, StationStateInfo};

    /// A serialized `SessionPayload::Review` with the given gate + parked station.
    fn payload(gate: Option<GateType>, parked: Option<&str>) -> String {
        let station_states = parked
            .map(|st| {
                vec![StationStateInfo {
                    station: st.into(),
                    merged_into_main: false,
                    status: None,
                    phase: None,
                    started_at: None,
                    completed_at: None,
                    gate_entered_at: Some("t".into()),
                    gate_outcome: None,
                }]
            })
            .unwrap_or_default();
        let review = ReviewSessionPayload {
            session_id: "s".into(),
            run_slug: Some("quiet-canyon".into()),
            gate_type: gate,
            station_states,
            ..Default::default()
        };
        serde_json::to_string(&SessionPayload::Review(review)).unwrap()
    }

    #[test]
    fn gate_target_reads_a_parked_review_only() {
        assert_eq!(
            gate_target(&payload(Some(GateType::Ask), Some("build"))),
            Some(("quiet-canyon".into(), "build".into()))
        );
        // No gate_type → not at a gate.
        assert_eq!(gate_target(&payload(None, Some("build"))), None);
        // Gated but no pinpointable parked station → empty station label.
        assert_eq!(
            gate_target(&payload(Some(GateType::Ask), None)),
            Some(("quiet-canyon".into(), String::new()))
        );
        // Non-session JSON → None.
        assert_eq!(gate_target("not json"), None);
    }

    #[test]
    fn gate_watcher_notifies_on_entry_only() {
        let mut w = GateWatcher::default();
        // The first payload is a SILENT baseline, even when already gated.
        assert_eq!(w.observe(&payload(Some(GateType::Ask), Some("build"))), None);
        // Staying parked at the same gate → no repeat.
        assert_eq!(w.observe(&payload(Some(GateType::Ask), Some("build"))), None);
        // Gate clears...
        assert_eq!(w.observe(&payload(None, None)), None);
        // ...then a new gate opens → notify with that station.
        assert_eq!(
            w.observe(&payload(Some(GateType::Ask), Some("prove"))),
            Some(("darkrun · quiet-canyon".into(), "Prove needs your decision.".into()))
        );
    }

    #[test]
    fn gate_watcher_notifies_entering_from_an_ungated_baseline() {
        let mut w = GateWatcher::default();
        assert_eq!(w.observe(&payload(None, None)), None); // baseline: ungated
        assert!(w.observe(&payload(Some(GateType::Ask), Some("frame"))).is_some());
    }

    #[test]
    fn wrap_payload_first_is_snapshot_then_updates() {
        let snap = wrap_payload(0, r#"{"station":"frame"}"#, true);
        assert_eq!(
            snap,
            ServerFrame::Snapshot { seq: 0, payload: serde_json::json!({"station": "frame"}) }
        );
        let upd = wrap_payload(1, r#"{"station":"build"}"#, false);
        assert_eq!(
            upd,
            ServerFrame::Update { seq: 1, payload: serde_json::json!({"station": "build"}) }
        );
        // Non-JSON payloads degrade to null rather than dropping the frame.
        assert_eq!(
            wrap_payload(2, "not json", false),
            ServerFrame::Update { seq: 2, payload: Value::Null }
        );
    }

    // ─── Command dedup ────────────────────────────────────────────────────────

    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The decision core: a new key executes, a re-seen COMPLETED key replays the
    /// cached ack, and the client's stable INSTANCE is part of the key.
    #[test]
    fn completed_command_replays_cached_ack_and_keys_on_instance() {
        let mut d = CommandDedup::new();
        let now = Instant::now();
        let key = ("i1".to_string(), "c1".to_string());
        assert!(matches!(d.begin(key.clone(), now), CmdAction::Execute));

        let ack = ServerFrame::Ack { id: "c1".into(), ok: true, error: None };
        d.complete(key.clone(), ack.clone(), now);

        // A redelivery of the same (instance, id) replays the cached ack.
        match d.begin(key, now) {
            CmdAction::Replay(f) => assert_eq!(f, ack),
            _ => panic!("a completed key must replay its cached ack, not execute"),
        }
        // A DIFFERENT instance (a second tab) reusing the same id still executes —
        // two tabs both starting their ids at `c0` must not collide.
        assert!(matches!(d.begin(("i2".into(), "c1".into()), now), CmdAction::Execute));
    }

    /// A duplicate that arrives while the original is still InFlight is dropped —
    /// it must not start a second exec.
    #[test]
    fn inflight_duplicate_is_dropped_not_re_executed() {
        let mut d = CommandDedup::new();
        let now = Instant::now();
        assert!(matches!(d.begin(("i1".into(), "c1".into()), now), CmdAction::Execute));
        assert!(matches!(d.begin(("i1".into(), "c1".into()), now), CmdAction::Drop));
        // Distinct ids each execute.
        assert!(matches!(d.begin(("i1".into(), "c2".into()), now), CmdAction::Execute));
    }

    /// The cache is bounded: past capacity, the oldest entries are evicted.
    #[test]
    fn dedup_map_is_bounded_by_capacity() {
        let mut d = CommandDedup::with_limits(3, DEDUP_TTL);
        let now = Instant::now();
        for i in 0..10u64 {
            d.begin((format!("i{i}"), "c".into()), now);
        }
        assert_eq!(d.len(), 3, "cache never exceeds its capacity");
        // The oldest key (i0) was evicted, so re-seeing it executes rather than
        // replaying — eviction only affects replays outside the redelivery window.
        assert!(matches!(d.begin(("i0".into(), "c".into()), now), CmdAction::Execute));
    }

    /// Entries older than the TTL are pruned on the next command.
    #[test]
    fn dedup_prunes_entries_past_ttl() {
        let ttl = Duration::from_secs(300);
        let mut d = CommandDedup::with_limits(4096, ttl);
        let t0 = Instant::now();
        d.begin(("i1".into(), "a".into()), t0);
        d.begin(("i2".into(), "b".into()), t0);
        assert_eq!(d.len(), 2);

        // A command far past the TTL prunes the two stale entries first.
        let later = t0 + ttl + Duration::from_secs(1);
        d.begin(("i3".into(), "c".into()), later);
        assert_eq!(d.len(), 1, "entries older than the TTL are pruned");
        // The pruned key executes again rather than replaying a stale ack.
        assert!(matches!(d.begin(("i1".into(), "a".into()), later), CmdAction::Execute));
    }

    /// A counting local engine: every request bumps `counter`, so a test can
    /// observe exactly how many times `exec_command` actually hit the engine.
    async fn counting_engine(counter: Arc<AtomicUsize>) -> String {
        let app = axum::Router::new().fallback(move || {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                "ok"
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    /// Drain the outbound queue until an `Ack` frame surfaces (unwrapping the
    /// relay `HostCmd::To` envelope), returning it.
    async fn recv_ack(out_rx: &mut mpsc::UnboundedReceiver<Message>) -> ServerFrame {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(2), out_rx.recv())
                .await
                .expect("an ack should be routed")
                .expect("the out channel stays open");
            let Message::Text(t) = msg else { continue };
            let Ok(HostCmd::To { data, .. }) = serde_json::from_str::<HostCmd>(&t) else {
                continue;
            };
            if let Ok(frame @ ServerFrame::Ack { .. }) = serde_json::from_str::<ServerFrame>(&data) {
                return frame;
            }
        }
    }

    fn cmd_frame(instance: &str, id: &str) -> String {
        serde_json::to_string(&ClientFrame::Cmd {
            instance: instance.into(),
            id: id.into(),
            command: ClientCommand::Advance { run: "r".into() },
        })
        .unwrap()
    }

    /// A duplicate (instance, id) executes EXACTLY ONCE, yet BOTH deliveries are
    /// acked (the replay serves the cached ack).
    #[tokio::test]
    async fn duplicate_command_executes_once_and_acks_both() {
        let counter = Arc::new(AtomicUsize::new(0));
        let base = counting_engine(Arc::clone(&counter)).await;
        let http = reqwest::Client::new();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let dedup = Arc::new(Mutex::new(CommandDedup::new()));

        let frame = cmd_frame("i1", "c1");

        // First delivery executes and acks.
        handle_client_frame(7, &frame, &base, http.clone(), out_tx.clone(), Arc::clone(&dedup));
        assert!(matches!(recv_ack(&mut out_rx).await, ServerFrame::Ack { ok: true, .. }));

        // Awaiting the first ack guarantees `complete` ran, so the redelivery hits
        // the Done path: it replays the cached ack and does NOT execute again.
        handle_client_frame(7, &frame, &base, http.clone(), out_tx.clone(), Arc::clone(&dedup));
        assert!(matches!(recv_ack(&mut out_rx).await, ServerFrame::Ack { ok: true, .. }));

        assert_eq!(counter.load(Ordering::SeqCst), 1, "exec_command must run exactly once");
    }

    /// The cross-reconnect regression test. The SAME (instance, id) arriving under
    /// DIFFERENT relay client ids — exactly what a client's resend of its pending
    /// set on a FRESH socket after a drop looks like — de-dupes to a SINGLE exec.
    /// The old key (the relay's per-socket client id) would have keyed the resend
    /// under a new id and double-applied the write; the stable instance closes the
    /// hole.
    #[tokio::test]
    async fn same_instance_across_reconnect_executes_once() {
        let counter = Arc::new(AtomicUsize::new(0));
        let base = counting_engine(Arc::clone(&counter)).await;
        let http = reqwest::Client::new();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let dedup = Arc::new(Mutex::new(CommandDedup::new()));

        // Same page load (instance "i1") and command id "c0", delivered on two
        // DIFFERENT relay connections: client 1, then client 2 after a reconnect.
        let frame = cmd_frame("i1", "c0");
        handle_client_frame(1, &frame, &base, http.clone(), out_tx.clone(), Arc::clone(&dedup));
        assert!(matches!(recv_ack(&mut out_rx).await, ServerFrame::Ack { ok: true, .. }));

        // The reconnect resend under the NEW client id must NOT re-execute — the
        // stable instance collapses it to the cached ack.
        handle_client_frame(2, &frame, &base, http.clone(), out_tx.clone(), Arc::clone(&dedup));
        assert!(matches!(recv_ack(&mut out_rx).await, ServerFrame::Ack { ok: true, .. }));

        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "a resend on a fresh relay connection must not double-apply"
        );
    }

    /// Two DISTINCT ids from the same instance each execute and each ack.
    #[tokio::test]
    async fn two_distinct_ids_each_execute() {
        let counter = Arc::new(AtomicUsize::new(0));
        let base = counting_engine(Arc::clone(&counter)).await;
        let http = reqwest::Client::new();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let dedup = Arc::new(Mutex::new(CommandDedup::new()));

        handle_client_frame(7, &cmd_frame("i1", "c1"), &base, http.clone(), out_tx.clone(), Arc::clone(&dedup));
        assert!(matches!(recv_ack(&mut out_rx).await, ServerFrame::Ack { ok: true, .. }));
        handle_client_frame(7, &cmd_frame("i1", "c2"), &base, http.clone(), out_tx.clone(), Arc::clone(&dedup));
        assert!(matches!(recv_ack(&mut out_rx).await, ServerFrame::Ack { ok: true, .. }));

        assert_eq!(counter.load(Ordering::SeqCst), 2, "distinct ids each execute");
    }

    /// Two DISTINCT instances (two browser tabs) emitting the SAME command id —
    /// both number their ids from `c0` — each execute; the instance keeps them
    /// apart, so id reuse across tabs never swallows the second write.
    #[tokio::test]
    async fn two_tabs_reusing_the_same_id_each_execute() {
        let counter = Arc::new(AtomicUsize::new(0));
        let base = counting_engine(Arc::clone(&counter)).await;
        let http = reqwest::Client::new();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let dedup = Arc::new(Mutex::new(CommandDedup::new()));

        handle_client_frame(1, &cmd_frame("tab-a", "c0"), &base, http.clone(), out_tx.clone(), Arc::clone(&dedup));
        assert!(matches!(recv_ack(&mut out_rx).await, ServerFrame::Ack { ok: true, .. }));
        handle_client_frame(2, &cmd_frame("tab-b", "c0"), &base, http.clone(), out_tx.clone(), Arc::clone(&dedup));
        assert!(matches!(recv_ack(&mut out_rx).await, ServerFrame::Ack { ok: true, .. }));

        assert_eq!(counter.load(Ordering::SeqCst), 2, "distinct instances each execute");
    }
}
