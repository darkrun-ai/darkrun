//! Shared server state: the session registry, the WebSocket connection
//! registry, and the resource-limit configuration.
//!
//! The HTTP server is dependency-light at the domain edge: it serves
//! interactive [`SessionPayload`]s out of an in-memory registry that the
//! manager (darkrun-mcp) populates, while reading feedback off the
//! filesystem via [`darkrun_core::StateStore`]. Keeping the session source in
//! memory (rather than re-deriving it from disk on every request) keeps the
//! live-update WebSocket cheap.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use darkrun_api::{Proof, SessionPayload, SessionStatus};
use darkrun_core::StateStore;
use tokio::sync::broadcast;

/// Default per-IP request ceiling per rate-limit window (60 requests / minute).
pub const DEFAULT_RATE_LIMIT_PER_MIN: u64 = 60;
/// Default cap on concurrent TCP connections.
pub const DEFAULT_MAX_CONNECTIONS: usize = 256;
/// Default cap on concurrent WebSocket sessions.
pub const DEFAULT_MAX_WS_SESSIONS: usize = 128;
/// Capacity of each session's broadcast channel (buffered server frames).
const WS_CHANNEL_CAPACITY: usize = 64;
/// How long after the desktop app's last connection drops it is still treated as
/// merely *lost* (a backgrounded tab, a network blip) rather than *closed* — the
/// presence grace window. Within it, the engine should not relaunch the app.
pub const PRESENCE_GRACE_MS: u64 = 15_000;

/// The desktop app's connection presence, with a grace window so a momentary
/// disconnect doesn't read as "closed" (F5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// At least one client is connected right now.
    Live,
    /// Was connected, dropped within the last [`PRESENCE_GRACE_MS`] — likely a
    /// blip or a backgrounded tab; may reattach. Don't relaunch yet.
    Lost,
    /// Was connected and has been gone past the grace window — the app is closed.
    Closed,
    /// No client has ever connected this process — the app hasn't opened yet.
    NeverAttached,
}

impl Presence {
    /// Whether the engine should consider the app present (live or in grace) —
    /// the relaunch decision uses this so a brief drop doesn't respawn the app.
    pub fn is_present(self) -> bool {
        matches!(self, Presence::Live | Presence::Lost)
    }
}

/// Process-wide desktop-presence tracker: records connection transitions with
/// timestamps so [`SessionRegistry::presence`] can apply the grace window.
#[derive(Default)]
struct PresenceTracker {
    ever_connected: AtomicBool,
    /// Epoch-ms when the connection count last fell to zero. `0` = never lost.
    last_lost_ms: AtomicU64,
}

/// Per-session presence state, mirroring [`PresenceTracker`] but scoped to one
/// session id (the run slug, for run sessions). Existence in the map IS the
/// "ever connected" bit: an entry is created on the first subscriber and never
/// removed, so a session that lost its last subscriber can still report the
/// grace window.
#[derive(Default)]
struct SessionPresence {
    /// Live subscriber count for this session.
    count: u64,
    /// Epoch-ms when `count` last fell to zero. `0` = never lost.
    last_lost_ms: u64,
}

/// The run a session payload belongs to, across every variant. `darkrun-api`
/// exposes no single accessor (each payload carries its own `run_slug` field),
/// so this projects them uniformly for the run-scoped [`SessionRegistry::retire_run`]
/// sweep. The `View` variant's `run_slug` is a bare `String`; the rest are
/// `Option<String>`.
fn payload_run_slug(payload: &SessionPayload) -> Option<&str> {
    match payload {
        SessionPayload::Review(p) => p.run_slug.as_deref(),
        SessionPayload::Question(p) => p.run_slug.as_deref(),
        SessionPayload::Direction(p) => p.run_slug.as_deref(),
        SessionPayload::Picker(p) => p.run_slug.as_deref(),
        SessionPayload::View(p) => Some(p.run_slug.as_str()),
        SessionPayload::VisualReview(p) => p.run_slug.as_deref(),
        SessionPayload::Proof(p) => p.run_slug.as_deref(),
    }
}

/// Current epoch milliseconds.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
/// Global request-body ceiling (1 MiB). Oversize bodies are rejected `413`
/// before a handler runs, bounding memory per request.
pub const DEFAULT_BODY_MAX_BYTES: usize = 1_048_576;

/// Resource limits applied by the middleware stack and WebSocket upgrade path.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Per-IP request ceiling per minute. Applied only in remote mode.
    pub rate_limit_per_min: u64,
    /// Maximum concurrent TCP connections.
    pub max_connections: usize,
    /// Maximum concurrent WebSocket sessions.
    pub max_ws_sessions: usize,
    /// Whether the server is reachable beyond loopback. CORS + rate limiting
    /// only engage when `true`, reflecting the local-vs-remote split.
    pub remote: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            rate_limit_per_min: DEFAULT_RATE_LIMIT_PER_MIN,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            max_ws_sessions: DEFAULT_MAX_WS_SESSIONS,
            remote: false,
        }
    }
}

/// One registered session plus its live-update broadcast channel.
struct SessionEntry {
    payload: SessionPayload,
    tx: broadcast::Sender<String>,
}

/// The result of [`SessionRegistry::await_decision`].
#[derive(Debug, Clone)]
pub enum AwaitOutcome {
    /// The operator acted; carries the resolved payload (the decision/answer).
    /// Boxed: a [`SessionPayload`] is large (~1.3 KiB), and every other variant
    /// is data-free, so boxing keeps the enum small for the common no-payload
    /// outcomes (`TimedOut`/`Unknown`/`Gone`).
    Resolved(Box<SessionPayload>),
    /// The timeout elapsed before a decision (the caller should re-await or fall
    /// back to elicitation).
    TimedOut,
    /// No session with that id exists.
    Unknown,
    /// The session was removed while awaiting (channel closed).
    Gone,
}

/// In-memory registry of interactive sessions, keyed by `session_id`.
///
/// Clonable and `Send + Sync`: every clone shares the same backing map, so the
/// manager and the HTTP handlers observe the same sessions. Mutations push
/// a fresh JSON frame to any WebSocket subscribed to that session.
#[derive(Clone, Default)]
pub struct SessionRegistry {
    inner: Arc<Mutex<HashMap<String, SessionEntry>>>,
    ws_session_count: Arc<AtomicU64>,
    presence: Arc<PresenceTracker>,
    /// Per-session subscriber presence, keyed by the WS subscription's session
    /// id (the run slug, for run sessions). The GLOBAL count above answers "is
    /// anything connected"; this answers "is anyone watching THIS run", which
    /// is what gate surfacing needs: a desktop viewing run A must not make a
    /// gate on run B hold for a viewer that isn't there.
    presence_by_session: Arc<Mutex<HashMap<String, SessionPresence>>>,
    /// Per-session set of device tokens that have ACKed a gate push for it. A
    /// device woken by a push POSTs `/api/push/ack` to confirm receipt, which
    /// lands here. The gate logic reads it as the "high-confidence live surface"
    /// signal — push delivered AND the app answered — so `await_decision` can
    /// block knowing a human can act, rather than guessing from a fire-and-forget
    /// push. Held behind the same Arc-shared posture as `inner` so the HTTP ack
    /// handler and the await tool observe the same acks. iOS acks are best-effort
    /// (silent-push throttling), so presence remains the stronger signal.
    acks: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    /// Optional durability hook the engine installs: invoked on every upsert
    /// (raise AND answer) with the payload, so interactive sessions persist to
    /// disk without the HTTP answer handlers needing to know how. Shared across
    /// clones (the engine installs it once, after construction).
    persist: Arc<Mutex<Option<PersistHook>>>,
    /// Whether a DURABLE gate-decision path exists for this registry: flipped by
    /// the engine when it installs the HTTP layer's gate-decider hook (see
    /// `AppState::with_gate_decider`). The MCP advance only HOLDS at an operator
    /// gate when this is set — a context where no decision can ever land (a bare
    /// registry in tests, an engine-less embedding) keeps the immediate-return
    /// contract instead of blocking on an answer that cannot arrive.
    durable_decisions: Arc<AtomicBool>,
}

/// A durability callback: persist a session payload (e.g. to the run's
/// `interactive/` dir). See [`SessionRegistry::on_persist`].
pub type PersistHook = Arc<dyn Fn(&SessionPayload) + Send + Sync>;

impl SessionRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the durability hook (see [`PersistHook`]). Shared across every
    /// clone of this registry, so handlers that hold a clone persist too.
    pub fn on_persist(&self, hook: PersistHook) {
        *self.persist.lock().expect("session registry poisoned") = Some(hook);
    }

    /// Mark that a DURABLE gate-decision path exists (see `durable_decisions`).
    /// The engine calls this when it installs the gate-decider hook; shared
    /// across every clone of this registry.
    pub fn enable_durable_decisions(&self) {
        self.durable_decisions.store(true, Ordering::Release);
    }

    /// Whether a durable gate-decision path exists — the precondition for the
    /// MCP advance to HOLD at an operator gate (see `durable_decisions`).
    pub fn durable_decisions_enabled(&self) -> bool {
        self.durable_decisions.load(Ordering::Acquire)
    }

    /// Run the persist hook for `payload`, if one is installed.
    fn persist(&self, payload: &SessionPayload) {
        let hook = self
            .persist
            .lock()
            .expect("session registry poisoned")
            .clone();
        if let Some(hook) = hook {
            hook(payload);
        }
    }

    /// Insert or replace a session. Any subscribed WebSocket receives the new
    /// payload as a JSON frame immediately.
    pub fn upsert(&self, payload: SessionPayload) {
        let id = payload.session_id().to_string();
        self.upsert_under(&id, payload);
    }

    /// Insert or replace a session under an EXPLICIT id, regardless of the
    /// payload's own `session_id`. The mirror mechanism: a question raised under
    /// `q-NN` is also written under the run slug so a desktop subscribed to the
    /// run's channel renders it live — while the payload still names `q-NN`, so
    /// the operator's answer routes back to the canonical session. The persist
    /// hook fires once, for the payload (not per-mirror), so disk isn't written
    /// twice for one logical session.
    pub fn upsert_under(&self, id: &str, payload: SessionPayload) {
        let frame = serde_json::to_string(&payload).ok();
        // Persist only when storing under the payload's own id (the canonical
        // write) — mirrors are view-only and share the same on-disk record.
        if id == payload.session_id() {
            self.persist(&payload);
        }
        let mut guard = self.inner.lock().expect("session registry poisoned");
        let entry = guard.entry(id.to_string()).or_insert_with(|| SessionEntry {
            payload: payload.clone(),
            tx: broadcast::channel(WS_CHANNEL_CAPACITY).0,
        });
        entry.payload = payload;
        if let Some(frame) = frame {
            // Ignore send errors: no subscribers is fine.
            let _ = entry.tx.send(frame);
        }
    }

    /// Fetch a session payload by id.
    pub fn get(&self, id: &str) -> Option<SessionPayload> {
        let guard = self.inner.lock().expect("session registry poisoned");
        guard.get(id).map(|e| e.payload.clone())
    }

    /// Reset a decision-bearing session's status back to `Pending`, so a
    /// subsequent [`await_decision`](Self::await_decision) genuinely BLOCKS
    /// instead of returning `Resolved` off a stale flip. The self-heal the
    /// advance gate-hold uses: when a `Resolved` outcome's re-tick STILL lands
    /// on the same operator gate (the on-disk gate never cleared — e.g. the SPA
    /// wake `POST /api/advance/:id` flipped the session `Decided` without
    /// landing the decision durably), the in-memory resolution is stale and
    /// must be consumed so the next hold waits for a REAL decision rather than
    /// spinning. Returns whether a decision-bearing session with that id existed
    /// (and was reset); a no-op `false` on an unknown id or a display-only
    /// variant with no status. Broadcasts the reset frame like any upsert.
    pub fn mark_pending(&self, id: &str) -> bool {
        let Some(mut payload) = self.get(id) else {
            return false;
        };
        if payload.status().is_none() {
            return false;
        }
        payload.set_status(SessionStatus::Pending);
        self.upsert_under(id, payload);
        true
    }

    /// Mint the next session id for the given kind `prefix` (`q`/`d`/`p`),
    /// scanning the live registry so ids stay unique and monotonic within the
    /// process. Format: `{prefix}-NN` (zero-padded to two digits).
    ///
    /// This is the in-memory replacement for the old on-disk `session.json`
    /// id-minting: the manager (darkrun-mcp) calls it to label a session before
    /// upserting it, so the desktop app sees stable `/api/session/:id` paths.
    pub fn next_session_id(&self, prefix: &str) -> String {
        let want = format!("{prefix}-");
        let guard = self.inner.lock().expect("session registry poisoned");
        let max = guard
            .keys()
            .filter_map(|k| k.strip_prefix(&want))
            .filter_map(|n| n.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("{prefix}-{:02}", max + 1)
    }

    /// Whether a session with the given id exists (drives the heartbeat probe).
    pub fn contains(&self, id: &str) -> bool {
        let guard = self.inner.lock().expect("session registry poisoned");
        guard.contains_key(id)
    }

    /// The number of live WebSocket subscribers across all sessions — a proxy
    /// for "is the desktop app connected". `0` means nothing is listening, so the
    /// engine should launch the desktop app.
    pub fn live_connections(&self) -> u64 {
        self.ws_session_count.load(Ordering::Acquire)
    }

    /// The desktop app's connection presence, with a grace window so a momentary
    /// disconnect doesn't read as "closed" (F5). The relaunch decision should use
    /// `presence().is_present()` rather than `live_connections() > 0`, so a
    /// backgrounded tab or a network blip doesn't respawn the app.
    pub fn presence(&self) -> Presence {
        self.presence_at(now_ms())
    }

    /// [`presence`](Self::presence) evaluated at an explicit `now` (epoch ms) —
    /// the clock seam so the grace window is testable without sleeping.
    fn presence_at(&self, now: u64) -> Presence {
        if self.ws_session_count.load(Ordering::Acquire) > 0 {
            return Presence::Live;
        }
        if !self.presence.ever_connected.load(Ordering::Acquire) {
            return Presence::NeverAttached;
        }
        let lost = self.presence.last_lost_ms.load(Ordering::Acquire);
        if lost != 0 && now.saturating_sub(lost) < PRESENCE_GRACE_MS {
            Presence::Lost
        } else {
            Presence::Closed
        }
    }

    /// Presence scoped to ONE session id (the run slug, for run sessions), with
    /// the same grace-window semantics as the global [`presence`](Self::presence).
    /// This is what gate surfacing consults: a desktop watching run A is
    /// `NeverAttached` from run B's point of view, so B's gate still LAUNCHES.
    pub fn presence_for(&self, session: &str) -> Presence {
        self.presence_for_at(session, now_ms())
    }

    /// [`presence_for`](Self::presence_for) at an explicit `now` (epoch ms),
    /// the clock seam so the per-session grace window is testable too.
    fn presence_for_at(&self, session: &str, now: u64) -> Presence {
        let guard = self
            .presence_by_session
            .lock()
            .expect("session registry poisoned");
        // No entry: no subscriber has ever attached to this session.
        let Some(entry) = guard.get(session) else {
            return Presence::NeverAttached;
        };
        if entry.count > 0 {
            return Presence::Live;
        }
        if entry.last_lost_ms != 0 && now.saturating_sub(entry.last_lost_ms) < PRESENCE_GRACE_MS {
            Presence::Lost
        } else {
            Presence::Closed
        }
    }

    /// Remove a session and drop its broadcast channel (closing subscribers).
    pub fn remove(&self, id: &str) -> Option<SessionPayload> {
        let mut guard = self.inner.lock().expect("session registry poisoned");
        guard.remove(id).map(|e| e.payload)
    }

    /// Retire every in-memory session belonging to a run — its review session
    /// (keyed by the run slug), every interactive/derived session that names the
    /// run (question / direction / picker / view / visual-review / proof), and the
    /// `current` focus pointer when it points at this run.
    ///
    /// Called when a run is RESET or ARCHIVED: the on-disk run is gone, but its
    /// sessions would otherwise linger in the registry and a subscribed desktop
    /// would sit on a PENDING GATE for a run that no longer exists — an
    /// unresolvable zombie the operator can neither approve nor dismiss. Removal
    /// drops each session's broadcast channel, so a subscriber's stream closes
    /// (and any in-flight `await_decision` returns `Gone`) — the signal for the
    /// client to re-render "run was reset" rather than the stale gate, and for a
    /// subsequent `GET /api/session/:id` to `404`. Returns how many were retired.
    ///
    /// Idempotent: a run with no live sessions retires zero.
    pub fn retire_run(&self, slug: &str) -> usize {
        // Collect the ids to drop while holding the lock, then remove them
        // afterwards (remove() re-locks, so it can't run inside this guard).
        let ids: Vec<String> = {
            let guard = self.inner.lock().expect("session registry poisoned");
            guard
                .iter()
                .filter(|(id, entry)| {
                    id.as_str() == slug || payload_run_slug(&entry.payload) == Some(slug)
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        ids.iter().filter(|id| self.remove(id).is_some()).count()
    }

    /// Subscribe to live-update frames for a session, creating the entry's
    /// channel lazily. Returns `None` if the session does not exist.
    pub fn subscribe(&self, id: &str) -> Option<broadcast::Receiver<String>> {
        let guard = self.inner.lock().expect("session registry poisoned");
        guard.get(id).map(|e| e.tx.subscribe())
    }

    /// Record that `token` (an FCM device token) ACKed the gate push for session
    /// `id` — the device confirming, on receipt, that the push landed. Idempotent:
    /// the same device re-acking is a no-op (it's a set membership). Drives the
    /// "high-confidence live surface" branch of the notify-and-await decision.
    pub fn record_ack(&self, id: &str, token: &str) {
        let mut guard = self.acks.lock().expect("session registry poisoned");
        guard
            .entry(id.to_string())
            .or_default()
            .insert(token.to_string());
    }

    /// How many distinct devices have ACKed a gate push for session `id`. `0`
    /// means no device confirmed receipt (fall back to presence / the URL).
    pub fn ack_count(&self, id: &str) -> usize {
        let guard = self.acks.lock().expect("session registry poisoned");
        guard.get(id).map_or(0, HashSet::len)
    }

    /// Whether at least one device has ACKed a gate push for session `id` — the
    /// boolean the gate logic reads when deciding whether a push reached a live
    /// surface it can confidently `await` against.
    pub fn has_ack(&self, id: &str) -> bool {
        self.ack_count(id) > 0
    }

    /// Block until session `id` is RESOLVED (the operator decided/answered — see
    /// [`SessionPayload::resolved`]), or `timeout` elapses. The server half of
    /// the notify-and-await gate model: surface the gate (push / URL elicitation /
    /// local launch) elsewhere, then hold here until the `/review/:id/decide`
    /// POST flips the session and broadcasts the frame. Transport-agnostic — the
    /// decision lands the same whether the operator is local (loopback) or remote
    /// (relay). Re-reads the authoritative payload on every frame, so a missed/
    /// lagged broadcast can't strand the waiter.
    pub async fn await_decision(&self, id: &str, timeout: std::time::Duration) -> AwaitOutcome {
        match self.get(id) {
            Some(p) if p.resolved() => return AwaitOutcome::Resolved(Box::new(p)),
            Some(_) => {}
            None => return AwaitOutcome::Unknown,
        }
        let Some(mut rx) = self.subscribe(id) else {
            return AwaitOutcome::Unknown;
        };
        // Re-check after subscribing: a decision that landed between the get()
        // above and this subscribe() would not arrive on `rx`.
        if let Some(p) = self.get(id) {
            if p.resolved() {
                return AwaitOutcome::Resolved(Box::new(p));
            }
        }
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return AwaitOutcome::TimedOut;
            }
            match tokio::time::timeout(remaining, rx.recv()).await {
                // A frame (or a lag, meaning we missed frames) — re-read the
                // authoritative payload and check whether it resolved.
                Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                    if let Some(p) = self.get(id) {
                        if p.resolved() {
                            return AwaitOutcome::Resolved(Box::new(p));
                        }
                    } else {
                        return AwaitOutcome::Gone;
                    }
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => return AwaitOutcome::Gone,
                Err(_) => return AwaitOutcome::TimedOut,
            }
        }
    }

    /// Try to reserve a WebSocket slot, honouring `max_ws_sessions`. Returns a
    /// guard that releases the slot on drop, or `None` if the cap is hit.
    pub fn try_acquire_ws_slot(&self, max: usize) -> Option<WsSlot> {
        self.acquire_ws_slot(None, max)
    }

    /// Like [`try_acquire_ws_slot`](Self::try_acquire_ws_slot) but attributed
    /// to a session id, so the subscriber also counts toward that session's
    /// [`presence_for`](Self::presence_for) as well as the global count. The
    /// WS upgrade path uses this (it knows which session it subscribes to).
    pub fn try_acquire_ws_slot_for(&self, session: &str, max: usize) -> Option<WsSlot> {
        self.acquire_ws_slot(Some(session), max)
    }

    /// The shared slot-acquire: bump the global count (cap-checked), and when
    /// `session` is known, the per-session count too. The returned guard
    /// releases both on drop.
    fn acquire_ws_slot(&self, session: Option<&str>, max: usize) -> Option<WsSlot> {
        loop {
            let current = self.ws_session_count.load(Ordering::Acquire);
            if current as usize >= max {
                return None;
            }
            if self
                .ws_session_count
                .compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                // The app is (re)attached — mark it ever-connected so a later
                // drop is a *loss*, not "never opened" (F5).
                self.presence.ever_connected.store(true, Ordering::Release);
                if let Some(id) = session {
                    let mut guard = self
                        .presence_by_session
                        .lock()
                        .expect("session registry poisoned");
                    guard.entry(id.to_string()).or_default().count += 1;
                }
                return Some(WsSlot {
                    counter: Arc::clone(&self.ws_session_count),
                    presence: Arc::clone(&self.presence),
                    session: session.map(str::to_string),
                    presence_by_session: Arc::clone(&self.presence_by_session),
                });
            }
        }
    }
}

/// RAII guard for a reserved WebSocket session slot. Releases on drop.
pub struct WsSlot {
    counter: Arc<AtomicU64>,
    presence: Arc<PresenceTracker>,
    /// The session this slot was attributed to (see `try_acquire_ws_slot_for`);
    /// `None` for a slot acquired without session attribution.
    session: Option<String>,
    presence_by_session: Arc<Mutex<HashMap<String, SessionPresence>>>,
}

impl Drop for WsSlot {
    fn drop(&mut self) {
        // Per-session release first: when this was the session's last
        // subscriber, stamp its loss time so its grace window starts. Tolerate
        // a poisoned lock (never panic in Drop); the entry is retained so the
        // session keeps reporting Lost/Closed rather than NeverAttached.
        if let Some(id) = self.session.take() {
            let mut guard = match self.presence_by_session.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(entry) = guard.get_mut(&id) {
                entry.count = entry.count.saturating_sub(1);
                if entry.count == 0 {
                    entry.last_lost_ms = now_ms();
                }
            }
        }
        // `fetch_sub` returns the PRIOR value; if it was 1 the count just fell to
        // zero — stamp the loss time so the grace window starts (F5).
        if self.counter.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.presence.last_lost_ms.store(now_ms(), Ordering::Release);
        }
    }
}

/// One attached proof plus the station it was measured at.
#[derive(Clone)]
struct ProofEntry {
    proof: Proof,
    station: Option<String>,
}

/// In-memory registry of run-scoped objective-evidence [`Proof`]s, keyed by run
/// slug. Populated by the Prove station's `POST /api/proof/:run`; read back by
/// the desktop app's `GET /api/proof/:run`.
///
/// Clonable + `Send + Sync` (shares the backing map across clones), mirroring
/// the [`SessionRegistry`] posture so the manager and HTTP handlers observe the
/// same proofs without a disk round-trip.
#[derive(Clone, Default)]
pub struct ProofRegistry {
    inner: Arc<Mutex<HashMap<String, ProofEntry>>>,
}

impl ProofRegistry {
    /// Create an empty proof registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach (or replace) the proof for a run, with its measured station.
    pub fn attach(&self, run: &str, proof: Proof, station: Option<String>) {
        let mut guard = self.inner.lock().expect("proof registry poisoned");
        guard.insert(run.to_string(), ProofEntry { proof, station });
    }

    /// Fetch a run's attached proof + station, if any.
    pub fn get(&self, run: &str) -> Option<(Proof, Option<String>)> {
        let guard = self.inner.lock().expect("proof registry poisoned");
        guard.get(run).map(|e| (e.proof.clone(), e.station.clone()))
    }
}

/// On-demand session builder: given a session id that misses the registry,
/// build it when it names something real (e.g. a run slug → its show session).
pub type SessionMaterializer = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Re-surface hook: called with a run slug after an operator resolves an
/// interactive session, to re-push the run's surface.
pub type SurfaceResolver = Arc<dyn Fn(&str) + Send + Sync>;

/// Durable gate-decision hook the engine installs: given a run slug, whether
/// the operator APPROVED, and any feedback, land the decision in the on-disk
/// StateStore the engine reads. Returns whether the land succeeded.
///
/// This is the bridge that keeps the durable gate write in sync with the
/// in-memory review session. `darkrun-http` cannot depend on `darkrun-mcp`
/// (that would be circular), so the engine (darkrun-mcp) installs a closure
/// wrapping its `checkpoint_decide` — the same posture as [`SurfaceResolver`]
/// and [`SessionMaterializer`]. The signature is `(run, approved, feedback)`.
/// The durable gate-decision hook the engine installs. Returns `Ok(())` when the
/// decision landed on disk, or `Err(reason)` when a gate guard REFUSED it (an
/// approve over open must/should, or a Prove approve with no measured evidence) so
/// the HTTP layer can surface the refusal instead of reporting a false success.
pub type GateDecider = Arc<dyn Fn(&str, bool, Option<String>) -> Result<(), String> + Send + Sync>;

/// The shared application state threaded through every handler.
#[derive(Clone)]
pub struct AppState {
    /// The in-memory interactive-session registry.
    pub sessions: SessionRegistry,
    /// The in-memory run-scoped proof registry.
    pub proofs: ProofRegistry,
    /// The filesystem state engine (used for feedback reads).
    pub store: Arc<StateStore>,
    /// Resource limits in effect.
    pub limits: Limits,
    /// Optional on-demand session builder the embedding engine installs: given
    /// a session id that MISSES the registry, build it when it names something
    /// real (e.g. a run slug → its show session). Lets a desktop open a run's
    /// review without waiting for the engine to tick first.
    pub materialize_session: Option<SessionMaterializer>,
    /// Optional re-surface hook the engine installs: called with a run slug
    /// after an operator resolves an interactive session (answers a question,
    /// picks a direction/option). Re-pushes the run's surface so the answered
    /// prompt is dismissed and the NEXT open one — or the review — takes its
    /// place on the desktop, without waiting for the agent's next tick.
    pub resolve_surface: Option<SurfaceResolver>,
    /// Optional durable gate-decision hook the engine installs: lands an
    /// operator's Approve / Request-changes into the on-disk StateStore (via
    /// the engine's `checkpoint_decide`), so `POST /review/:id/decide` writes
    /// the gate durably and not just to the in-memory session. Absent → no-op
    /// (the HTTP server keeps flipping the in-memory session only).
    pub gate_decider: Option<GateDecider>,
}

impl AppState {
    /// Build application state from a state store and resource limits.
    pub fn new(store: StateStore, limits: Limits) -> Self {
        Self {
            sessions: SessionRegistry::new(),
            proofs: ProofRegistry::new(),
            store: Arc::new(store),
            limits,
            materialize_session: None,
            resolve_surface: None,
            gate_decider: None,
        }
    }

    /// Install the on-demand session builder (see `materialize_session`).
    pub fn with_session_materializer(
        mut self,
        f: impl Fn(&str) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.materialize_session = Some(Arc::new(f));
        self
    }

    /// Install the re-surface hook (see `resolve_surface`).
    pub fn with_surface_resolver(
        mut self,
        f: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        self.resolve_surface = Some(Arc::new(f));
        self
    }

    /// Install the durable gate-decision hook (see `gate_decider`). The engine
    /// wraps its `checkpoint_decide` here so a review decision lands on disk.
    pub fn with_gate_decider(
        mut self,
        f: impl Fn(&str, bool, Option<String>) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.gate_decider = Some(Arc::new(f));
        self
    }

    /// Ensure `id` exists in the session registry, building it on demand via
    /// the installed materializer when absent. Returns whether it now exists.
    pub fn ensure_session(&self, id: &str) -> bool {
        if self.sessions.contains(id) {
            return true;
        }
        match &self.materialize_session {
            Some(build) => build(id) && self.sessions.contains(id),
            None => false,
        }
    }

    /// Re-surface the run channel after an interactive session resolved, via
    /// the installed hook. No-op when no run is known or no hook is installed.
    pub fn resolve_surface(&self, run: Option<&str>) {
        if let (Some(run), Some(hook)) = (run, &self.resolve_surface) {
            hook(run);
        }
    }

    /// Land an operator's gate decision durably via the installed gate-decider
    /// hook (see `gate_decider`). `Ok(())` when the decision landed (or when no
    /// hook is installed, the standalone HTTP server case, where the in-memory
    /// flip is authoritative and there is nothing to land). `Err(reason)` when a
    /// gate guard REFUSED it, so the caller surfaces the refusal rather than
    /// reporting a false success.
    pub fn decide_gate(
        &self,
        run: &str,
        approved: bool,
        feedback: Option<String>,
    ) -> Result<(), String> {
        match &self.gate_decider {
            Some(hook) => hook(run, approved, feedback),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn question(id: &str, status: &str) -> SessionPayload {
        serde_json::from_value(serde_json::json!({
            "session_type": "question", "session_id": id, "status": status
        }))
        .expect("minimal question payload")
    }

    fn review(id: &str, run: &str) -> SessionPayload {
        serde_json::from_value(serde_json::json!({
            "session_type": "review", "session_id": id, "status": "pending", "run_slug": run
        }))
        .expect("minimal review payload")
    }

    fn question_on(id: &str, run: &str) -> SessionPayload {
        serde_json::from_value(serde_json::json!({
            "session_type": "question", "session_id": id, "status": "pending", "run_slug": run
        }))
        .expect("minimal question payload")
    }

    /// INT-3 regression: retiring a RESET run drops its review session, every
    /// interactive session that names it, AND the `current` focus pointer aimed at
    /// it — while leaving a sibling run's sessions completely intact. Without this,
    /// a subscribed desktop sat on a pending gate for a run that no longer existed.
    #[test]
    fn retire_run_drops_only_the_named_runs_sessions() {
        let reg = SessionRegistry::new();
        // Run "one": its review session (keyed by slug), a question, and the shared
        // `current` focus pointing at it.
        reg.upsert_under("one", review("one", "one"));
        reg.upsert(question_on("q-01", "one"));
        reg.upsert_under("current", review("current", "one"));
        // Run "two": a review + a question that must SURVIVE the retire.
        reg.upsert_under("two", review("two", "two"));
        reg.upsert(question_on("q-02", "two"));

        let retired = reg.retire_run("one");
        assert_eq!(retired, 3, "one + q-01 + current retire");

        // The zombie session is gone (a subsequent GET would 404).
        assert!(reg.get("one").is_none());
        assert!(reg.get("q-01").is_none());
        assert!(reg.get("current").is_none());
        // The survivor run is untouched.
        assert!(reg.get("two").is_some());
        assert!(reg.get("q-02").is_some());

        // Idempotent: a second retire (or a run with no sessions) removes nothing.
        assert_eq!(reg.retire_run("one"), 0);
        assert_eq!(reg.retire_run("nope"), 0);
    }

    #[tokio::test]
    async fn await_decision_unblocks_when_the_session_resolves() {
        let reg = SessionRegistry::new();
        reg.upsert(question("q-01", "pending"));

        let r = reg.clone();
        let waiter =
            tokio::spawn(async move { r.await_decision("q-01", Duration::from_secs(5)).await });
        // Give the awaiter time to subscribe, then resolve from this task.
        tokio::time::sleep(Duration::from_millis(20)).await;
        reg.upsert(question("q-01", "answered"));

        match waiter.await.unwrap() {
            AwaitOutcome::Resolved(p) => assert_eq!(p.session_id(), "q-01"),
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn await_decision_returns_immediately_when_already_resolved() {
        let reg = SessionRegistry::new();
        reg.upsert(question("q-01", "answered"));
        assert!(matches!(
            reg.await_decision("q-01", Duration::from_secs(5)).await,
            AwaitOutcome::Resolved(_)
        ));
    }

    #[tokio::test]
    async fn await_decision_unknown_then_times_out() {
        let reg = SessionRegistry::new();
        assert!(matches!(
            reg.await_decision("nope", Duration::from_millis(10)).await,
            AwaitOutcome::Unknown
        ));
        reg.upsert(question("q-02", "pending"));
        assert!(matches!(
            reg.await_decision("q-02", Duration::from_millis(30)).await,
            AwaitOutcome::TimedOut
        ));
    }

    #[test]
    fn record_ack_is_idempotent_per_device_and_counts_distinct_devices() {
        let reg = SessionRegistry::new();
        assert_eq!(reg.ack_count("q-01"), 0);
        assert!(!reg.has_ack("q-01"));

        // The same device acking twice is one ack (set membership).
        reg.record_ack("q-01", "tok-a");
        reg.record_ack("q-01", "tok-a");
        assert_eq!(reg.ack_count("q-01"), 1);
        assert!(reg.has_ack("q-01"));

        // A second device adds a distinct ack; acks are scoped per session.
        reg.record_ack("q-01", "tok-b");
        assert_eq!(reg.ack_count("q-01"), 2);
        assert_eq!(reg.ack_count("q-02"), 0);
    }

    #[test]
    fn acks_are_shared_across_clones() {
        // The HTTP ack handler holds a clone of the registry; the await tool
        // holds another — both must observe the same acks.
        let reg = SessionRegistry::new();
        let clone = reg.clone();
        clone.record_ack("d-07", "tok");
        assert!(reg.has_ack("d-07"));
    }

    #[test]
    fn presence_starts_never_attached() {
        let reg = SessionRegistry::new();
        assert_eq!(reg.presence(), Presence::NeverAttached);
        assert!(!reg.presence().is_present());
    }

    #[test]
    fn presence_is_live_while_a_slot_is_held() {
        let reg = SessionRegistry::new();
        let slot = reg.try_acquire_ws_slot(8).expect("slot");
        assert_eq!(reg.presence(), Presence::Live);
        assert!(reg.presence().is_present());
        drop(slot);
    }

    #[test]
    fn presence_is_lost_within_grace_then_closed_after() {
        let reg = SessionRegistry::new();
        // Connect then drop → the loss time is stamped (~now).
        let slot = reg.try_acquire_ws_slot(8).expect("slot");
        drop(slot);
        let lost_at = reg.presence.last_lost_ms.load(Ordering::Acquire);
        assert!(lost_at > 0, "drop stamps the loss time");

        // Just after the drop: still LOST (within grace) and counts as present.
        let just_after = lost_at + 1;
        assert_eq!(reg.presence_at(just_after), Presence::Lost);
        assert!(reg.presence_at(just_after).is_present());

        // Past the grace window: CLOSED — the app is gone.
        let past_grace = lost_at + PRESENCE_GRACE_MS + 1;
        assert_eq!(reg.presence_at(past_grace), Presence::Closed);
        assert!(!reg.presence_at(past_grace).is_present());
    }

    #[test]
    fn reattaching_returns_to_live() {
        let reg = SessionRegistry::new();
        drop(reg.try_acquire_ws_slot(8).expect("slot")); // connect then lose
        let _slot = reg.try_acquire_ws_slot(8).expect("slot"); // reattach
        assert_eq!(reg.presence(), Presence::Live);
    }

    #[test]
    fn presence_for_is_per_session_not_global() {
        // The F12 scenario: a desktop watching run A must not make run B read
        // as attended. Two sessions, a subscriber on A only.
        let reg = SessionRegistry::new();
        let _a = reg.try_acquire_ws_slot_for("run-a", 8).expect("slot");
        assert_eq!(reg.presence_for("run-a"), Presence::Live);
        assert!(reg.presence_for("run-a").is_present());
        // Run B has never had a viewer: absent, so its gate LAUNCHES.
        assert_eq!(reg.presence_for("run-b"), Presence::NeverAttached);
        assert!(!reg.presence_for("run-b").is_present());
        // The global count still sees the one connection (unchanged consumers).
        assert_eq!(reg.presence(), Presence::Live);
        assert_eq!(reg.live_connections(), 1);
    }

    #[test]
    fn presence_for_mirrors_the_grace_window_per_session() {
        let reg = SessionRegistry::new();
        // Attach to A, then drop: A's loss is stamped and its grace window runs.
        let slot = reg.try_acquire_ws_slot_for("run-a", 8).expect("slot");
        drop(slot);
        let lost_at = {
            let guard = reg.presence_by_session.lock().unwrap();
            guard.get("run-a").expect("entry retained").last_lost_ms
        };
        assert!(lost_at > 0, "drop stamps the per-session loss time");

        // Within grace: LOST (still present); past it: CLOSED. Exactly the
        // global tracker's semantics, scoped to the session.
        assert_eq!(reg.presence_for_at("run-a", lost_at + 1), Presence::Lost);
        assert!(reg.presence_for_at("run-a", lost_at + 1).is_present());
        let past = lost_at + PRESENCE_GRACE_MS + 1;
        assert_eq!(reg.presence_for_at("run-a", past), Presence::Closed);
        // A session that never attached stays NeverAttached throughout.
        assert_eq!(reg.presence_for_at("run-b", past), Presence::NeverAttached);
    }

    #[test]
    fn session_slots_release_independently_and_share_clones() {
        let reg = SessionRegistry::new();
        let a1 = reg.try_acquire_ws_slot_for("run-a", 8).expect("slot");
        let a2 = reg.try_acquire_ws_slot_for("run-a", 8).expect("slot");
        // Clones observe the same per-session presence (Arc-shared map).
        assert_eq!(reg.clone().presence_for("run-a"), Presence::Live);
        // Dropping ONE of two subscribers keeps the session live.
        drop(a1);
        assert_eq!(reg.presence_for("run-a"), Presence::Live);
        // Dropping the last flips it out of Live (into the grace window).
        drop(a2);
        assert_eq!(reg.presence_for("run-a"), Presence::Lost);
        // An unattributed (global-only) slot never bleeds into a session.
        let _g = reg.try_acquire_ws_slot(8).expect("slot");
        assert_eq!(reg.presence_for("run-b"), Presence::NeverAttached);
    }

    #[test]
    fn decide_gate_invokes_the_installed_hook_with_its_args() {
        // The hook captures exactly what it was called with; decide_gate returns
        // whatever the hook returns.
        type Captured = Arc<Mutex<Option<(String, bool, Option<String>)>>>;
        let captured: Captured = Arc::new(Mutex::new(None));
        let sink = captured.clone();
        let state = AppState::new(StateStore::new("."), Limits::default()).with_gate_decider(
            move |run, approved, fb| {
                *sink.lock().unwrap() = Some((run.to_string(), approved, fb));
                Ok(())
            },
        );
        assert!(state.decide_gate("run-x", true, Some("looks good".into())).is_ok());
        assert_eq!(
            captured.lock().unwrap().clone(),
            Some(("run-x".to_string(), true, Some("looks good".to_string())))
        );
    }

    #[test]
    fn decide_gate_surfaces_a_hook_refusal() {
        // A gate guard that refuses (a Prove approve with no evidence) is carried
        // out as an Err so the HTTP layer can 409 instead of reporting success.
        let state = AppState::new(StateStore::new("."), Limits::default())
            .with_gate_decider(|_, _, _| Err("Prove needs measured evidence".to_string()));
        let err = state.decide_gate("run-x", true, None).unwrap_err();
        assert!(err.contains("evidence"), "{err}");
    }

    #[test]
    fn decide_gate_is_ok_noop_when_no_hook_is_installed() {
        // The standalone HTTP server (no engine) installs no hook — decide_gate is
        // a no-op Ok (the in-memory flip is authoritative, nothing to land).
        let state = AppState::new(StateStore::new("."), Limits::default());
        assert!(state.decide_gate("run-x", true, None).is_ok());
    }
}
