//! Relay-token broker — carries a browser-minted Firebase ID token to the
//! waiting CLI, so `/darkrun:darkrun-login` can store the token the engine dials
//! the relay with.
//!
//! The flow mirrors the OAuth broker, but the token is minted CLIENT-side (the
//! web app signs in with Firebase Auth in the browser) rather than exchanged
//! server-side:
//!
//! 1. the CLI generates a nonce and opens the browser to the web app's login
//!    with it;
//! 2. after Firebase sign-in the web app **POSTs** the ID token to
//!    `/auth/relay/deposit` under the nonce;
//! 3. the waiting CLI **GET**s `/auth/relay/claim/{nonce}` once to claim it.
//!
//! Single-use + TTL keep a parked token from leaking: a claim evicts the entry
//! (a replay misses), and an abandoned login expires on its own. The relay token
//! is just an opaque string here — its validity is the relay verifier's job.
//!
//! ## Two backends behind one [`RelayTokenStore`] trait
//!
//! deposit and claim run behind a [`RelayTokenStore`] so the store can be swapped
//! by environment, exactly like the FCM device registry ([`DeviceRegistry`]):
//!
//! * [`InMemoryRelayStore`] is the default for dev and tests: a `Mutex<HashMap>`
//!   with an injectable [`Clock`] and a timer sweep for abandoned deposits.
//! * [`FirestoreRelayStore`] persists each nonce as a Firestore document so ANY
//!   Cloud Run instance can serve deposit/claim (the in-memory map is per
//!   instance, which breaks once the service scales past one). park and claim run
//!   as REST read-modify-write transactions, and Firestore's native TTL GCs
//!   expired docs server-side, so no timer sweep is needed for that backend.
//!
//! [`DeviceRegistry`]: crate::push::DeviceRegistry

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::broker::{Clock, SystemClock, DEFAULT_TTL};
use crate::push::AccessTokenSource;

/// The future a [`RelayTokenStore`] method returns — boxed so the trait stays
/// object-safe (`dyn RelayTokenStore`) while a network-backed impl (Firestore)
/// does async I/O, without pulling in an async-trait dependency. Mirrors
/// [`RegistryFuture`](crate::push::RegistryFuture).
pub type RelayStoreFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Outcome of a [`RelayTokenStore::park`]. Distinguishing a real conflict from a
/// transient backend error matters on the login path: a Firestore blip must not
/// be reported to the browser as "already deposited" (which would silently fail
/// the CLI's claim with a 404), but as a retryable server error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParkOutcome {
    /// The token was parked — the first deposit wins and binds.
    Parked,
    /// A live (unexpired) token already holds this nonce; the deposit is refused
    /// (the session-fixation guard).
    Conflict,
    /// The store was unreachable (a transient backend error). The caller should
    /// retry rather than treat this as a conflict.
    Unavailable,
}

/// A single-use, TTL-bounded store of relay tokens keyed by nonce. Behind a trait
/// so the in-memory impl drives dev/tests offline and a Firestore-backed impl
/// makes deposit/claim horizontally scalable across Cloud Run instances. The
/// methods are async (boxed futures) because the Firestore impl talks to the REST
/// API over HTTP.
pub trait RelayTokenStore: Send + Sync {
    /// Park `token` under `nonce` — the FIRST deposit wins and binds. An expired
    /// entry frees the nonce so an abandoned login can retry. See [`ParkOutcome`].
    fn park<'a>(&'a self, nonce: &'a str, token: &'a str) -> RelayStoreFuture<'a, ParkOutcome>;

    /// Claim the token parked under `nonce`, evicting it. `None` if unknown,
    /// already claimed, or expired — single-use, so a replay can't resurrect it.
    fn claim<'a>(&'a self, nonce: &'a str) -> RelayStoreFuture<'a, Option<String>>;
}

/// A parked relay token and when it stops being claimable.
struct Entry {
    token: String,
    expires_at: Instant,
}

/// In-memory single-use store of relay tokens keyed by nonce. Cheap to clone
/// (clones share the backing store).
#[derive(Clone)]
pub struct InMemoryRelayStore {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
    ttl: Duration,
    clock: Arc<dyn Clock>,
}

impl Default for InMemoryRelayStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryRelayStore {
    /// A store with the default TTL and a real clock.
    pub fn new() -> Self {
        Self::with_clock(DEFAULT_TTL, Arc::new(SystemClock))
    }

    /// A store with an explicit TTL + clock — the test seam.
    pub fn with_clock(ttl: Duration, clock: Arc<dyn Clock>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ttl,
            clock,
        }
    }

    /// Drop every entry whose TTL has elapsed. Claims evict lazily; this sweeps
    /// abandoned (never-claimed) deposits so an attacker can't grow the map
    /// without bound by POSTing distinct nonces. Call it on a timer.
    pub fn sweep_expired(&self) {
        let now = self.clock.now();
        if let Ok(mut map) = self.inner.lock() {
            map.retain(|_, e| e.expires_at > now);
        }
    }

    /// The number of live entries — for tests/metrics.
    pub fn len(&self) -> usize {
        self.inner.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// Whether the store currently holds no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl RelayTokenStore for InMemoryRelayStore {
    /// First-deposit-wins with expired-reuse.
    ///
    /// First-deposit-wins is a session-fixation guard: the nonce carries no
    /// expected principal (the CLI mints it before anyone signs in), so we can't
    /// verify *whose* token this is here. What we CAN do is refuse to let a second
    /// deposit overwrite the first — otherwise an attacker who learns the nonce
    /// could race a second token in and rebind the victim's terminal to the
    /// attacker's account. Nonce secrecy (a CSPRNG nonce) is the primary defense;
    /// this closes the overwrite window behind it. An expired/claimed entry frees
    /// the nonce so an abandoned login can be retried. `ttl` still bounds an
    /// unclaimed entry's life.
    fn park<'a>(&'a self, nonce: &'a str, token: &'a str) -> RelayStoreFuture<'a, ParkOutcome> {
        let now = self.clock.now();
        let Ok(mut map) = self.inner.lock() else {
            return Box::pin(async { ParkOutcome::Unavailable });
        };
        if let Some(existing) = map.get(nonce) {
            if existing.expires_at > now {
                return Box::pin(async { ParkOutcome::Conflict }); // a live token already holds this nonce
            }
        }
        map.insert(nonce.to_string(), Entry { token: token.to_string(), expires_at: now + self.ttl });
        Box::pin(async { ParkOutcome::Parked })
    }

    fn claim<'a>(&'a self, nonce: &'a str) -> RelayStoreFuture<'a, Option<String>> {
        let now = self.clock.now();
        let claimed = self.inner.lock().ok().and_then(|mut map| {
            let entry = map.remove(nonce)?;
            (entry.expires_at > now).then_some(entry.token)
        });
        Box::pin(async move { claimed })
    }
}

// ── Firestore-backed store ──────────────────────────────────────────────────

/// The Firestore REST API base.
const FIRESTORE_BASE: &str = "https://firestore.googleapis.com/v1";

/// The collection holding parked relay tokens, one document per nonce.
const RELAY_COLLECTION: &str = "relayBroker";

/// How many times to retry a transaction that commits `ABORTED` (contention).
const MAX_TXN_ATTEMPTS: usize = 5;

/// Stable Firestore document id for a nonce: its SHA-256, hex-encoded. The
/// deposit endpoint is unauthenticated and takes an arbitrary nonce string, so
/// hashing keeps the document id fixed-length and path-safe (a raw nonce with a
/// `/` could otherwise steer the doc path at another collection). Mirrors the
/// device registry's per-token hashing.
fn doc_id(nonce: &str) -> String {
    Sha256::digest(nonce.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The Firestore resource name (no host/`/v1` prefix) for `nonce`'s document —
/// the value a write's `update.name` / `delete` takes.
fn doc_name(project_id: &str, nonce: &str) -> String {
    format!(
        "projects/{project_id}/databases/(default)/documents/{RELAY_COLLECTION}/{}",
        doc_id(nonce),
    )
}

/// The `:beginTransaction` body requesting a read-write transaction.
fn begin_transaction_body() -> serde_json::Value {
    serde_json::json!({ "options": { "readWrite": {} } })
}

/// One upsert write of a relay-token document: `{ token, expiresAt }`, with
/// `expires_at` an RFC3339 `timestampValue`.
fn relay_document_write(doc_name: &str, token: &str, expires_at: &str) -> serde_json::Value {
    serde_json::json!({
        "update": {
            "name": doc_name,
            "fields": {
                "token": { "stringValue": token },
                "expiresAt": { "timestampValue": expires_at },
            }
        }
    })
}

/// The park `:commit` body: within `transaction`, upsert the token document.
fn park_commit_body(
    transaction: &str,
    doc_name: &str,
    token: &str,
    expires_at: &str,
) -> serde_json::Value {
    serde_json::json!({
        "transaction": transaction,
        "writes": [ relay_document_write(doc_name, token, expires_at) ],
    })
}

/// The claim `:commit` body: within `transaction`, delete the token document
/// (single-use eviction on a live claim; cleanup on an expired one).
fn delete_commit_body(transaction: &str, doc_name: &str) -> serde_json::Value {
    serde_json::json!({
        "transaction": transaction,
        "writes": [ { "delete": doc_name } ],
    })
}

/// A `:commit` body that writes nothing — releases `transaction` for the paths
/// that decided not to mutate (a live nonce on park, an absent nonce on claim).
fn release_commit_body(transaction: &str) -> serde_json::Value {
    serde_json::json!({ "transaction": transaction, "writes": [] })
}

/// Encode the expiry of a token parked at `now` with lifetime `ttl` as a
/// Firestore `timestampValue` (RFC3339, second precision, `Z`).
fn expires_at_rfc3339(now: DateTime<Utc>, ttl: Duration) -> String {
    let expires = now + chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::zero());
    expires.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Parse a Firestore `timestampValue` (RFC3339) into a UTC instant.
fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc))
}

/// The `expiresAt` instant of a fetched relay-token document, if present + valid.
fn doc_expires_at(doc: &serde_json::Value) -> Option<DateTime<Utc>> {
    let s = doc.get("fields")?.get("expiresAt")?.get("timestampValue")?.as_str()?;
    parse_timestamp(s)
}

/// The `token` string of a fetched relay-token document, if present.
fn doc_token(doc: &serde_json::Value) -> Option<String> {
    Some(doc.get("fields")?.get("token")?.get("stringValue")?.as_str()?.to_string())
}

/// The transaction id from a `:beginTransaction` response.
fn transaction_id(resp: &serde_json::Value) -> Option<String> {
    Some(resp.get("transaction")?.as_str()?.to_string())
}

/// The outcome of a `:commit` — distinguished so callers can retry on `ABORTED`.
#[cfg(not(tarpaulin_include))]
enum Commit {
    /// The commit succeeded.
    Ok,
    /// `ABORTED` (HTTP 409) — a document read in the transaction changed under
    /// us; the transaction can be retried.
    Aborted,
    /// Any other failure — best-effort, so the caller gives up on this attempt.
    Failed,
}

/// A [`RelayTokenStore`] persisted in Firestore via the REST API, so deposit and
/// claim work from any Cloud Run instance. park/claim run as read-modify-write
/// transactions; wall-clock expiry (`expiresAt`) is authoritative on read
/// regardless of native-TTL GC lag.
pub struct FirestoreRelayStore<T: AccessTokenSource> {
    project_id: String,
    tokens: T,
    ttl: Duration,
    http: reqwest::Client,
}

impl<T: AccessTokenSource> FirestoreRelayStore<T> {
    /// A store for `project_id`, authorized by `tokens` (datastore-scoped), with
    /// the default TTL.
    pub fn new(project_id: impl Into<String>, tokens: T) -> Self {
        Self {
            project_id: project_id.into(),
            tokens,
            ttl: DEFAULT_TTL,
            http: reqwest::Client::new(),
        }
    }
}

#[cfg(not(tarpaulin_include))] // network I/O — request/commit shapes are unit-tested
impl<T: AccessTokenSource> FirestoreRelayStore<T> {
    /// The `documents` base URL for this database.
    fn documents_base(&self) -> String {
        format!("{FIRESTORE_BASE}/projects/{}/databases/(default)/documents", self.project_id)
    }

    /// Fetch a datastore-scoped access token, logging + returning `None` on error.
    async fn access(&self, what: &str) -> Option<String> {
        match self.tokens.access_token().await {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::warn!(error = %e, what, "Firestore token unavailable");
                None
            }
        }
    }

    /// Open a read-write transaction, returning its id.
    async fn begin_transaction(&self, access: &str) -> Option<String> {
        let url = format!("{}:beginTransaction", self.documents_base());
        let resp = match self
            .http
            .post(url)
            .bearer_auth(access)
            .json(&begin_transaction_body())
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::warn!(status = %r.status(), "Firestore beginTransaction rejected");
                return None;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Firestore beginTransaction failed");
                return None;
            }
        };
        match resp.json::<serde_json::Value>().await {
            Ok(v) => transaction_id(&v),
            Err(e) => {
                tracing::warn!(error = %e, "Firestore beginTransaction parse failed");
                None
            }
        }
    }

    /// Read the token document within `transaction`. `Ok(None)` = absent (404),
    /// `Ok(Some)` = present, `Err(())` = a read failure (abort this attempt).
    async fn get_doc(
        &self,
        access: &str,
        nonce: &str,
        transaction: &str,
    ) -> Result<Option<serde_json::Value>, ()> {
        let url = format!("{FIRESTORE_BASE}/{}", doc_name(&self.project_id, nonce));
        match self
            .http
            .get(url)
            .bearer_auth(access)
            .query(&[("transaction", transaction)])
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r.json::<serde_json::Value>().await.map(Some).map_err(|e| {
                tracing::warn!(error = %e, "Firestore relay read parse failed");
            }),
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => Ok(None),
            Ok(r) => {
                tracing::warn!(status = %r.status(), "Firestore relay read rejected");
                Err(())
            }
            Err(e) => {
                tracing::warn!(error = %e, "Firestore relay read failed");
                Err(())
            }
        }
    }

    /// Commit `body` (an already-built `:commit` payload).
    async fn commit(&self, access: &str, body: &serde_json::Value) -> Commit {
        let url = format!("{}:commit", self.documents_base());
        match self.http.post(url).bearer_auth(access).json(body).send().await {
            Ok(r) if r.status().is_success() => Commit::Ok,
            Ok(r) if r.status() == reqwest::StatusCode::CONFLICT => Commit::Aborted,
            Ok(r) => {
                tracing::warn!(status = %r.status(), "Firestore relay commit rejected");
                Commit::Failed
            }
            Err(e) => {
                tracing::warn!(error = %e, "Firestore relay commit failed");
                Commit::Failed
            }
        }
    }

    /// park as a read-modify-write transaction: first-deposit-wins with
    /// expired-reuse. Absent OR expired → upsert the new token (`Parked`); a live
    /// doc → write nothing (`Conflict`). Retries on `ABORTED`; a transient backend
    /// error is `Unavailable`, never a false `Conflict`.
    ///
    /// First-deposit-wins here rests on Firestore Native-mode read-write
    /// transactions tracking the read of the (absent or expired) doc key: two
    /// concurrent parks that both read "not live" serialize — one commits the
    /// upsert, the other commits `ABORTED` and retries. This is the load-bearing
    /// anti-rebind property; a move to Datastore-mode/optimistic semantics would
    /// silently regress it.
    async fn park_txn(&self, nonce: &str, token: &str) -> ParkOutcome {
        let Some(access) = self.access("park").await else {
            return ParkOutcome::Unavailable;
        };
        for _ in 0..MAX_TXN_ATTEMPTS {
            let Some(transaction) = self.begin_transaction(&access).await else {
                return ParkOutcome::Unavailable;
            };
            let existing = match self.get_doc(&access, nonce, &transaction).await {
                Ok(doc) => doc,
                Err(()) => {
                    let _ = self.commit(&access, &release_commit_body(&transaction)).await;
                    return ParkOutcome::Unavailable;
                }
            };
            let live = existing
                .as_ref()
                .and_then(doc_expires_at)
                .map(|e| e > Utc::now())
                .unwrap_or(false);
            let body = if live {
                release_commit_body(&transaction)
            } else {
                let expires = expires_at_rfc3339(Utc::now(), self.ttl);
                park_commit_body(&transaction, &doc_name(&self.project_id, nonce), token, &expires)
            };
            match self.commit(&access, &body).await {
                Commit::Ok if live => return ParkOutcome::Conflict, // a live token already holds it
                Commit::Ok => return ParkOutcome::Parked,
                Commit::Aborted => continue,
                Commit::Failed => return ParkOutcome::Unavailable,
            }
        }
        ParkOutcome::Unavailable // exhausted retries under contention
    }

    /// claim as a read-modify-write transaction: single-use. A live doc deletes
    /// and returns its token; an expired doc deletes and returns `None` (cleanup);
    /// an absent nonce returns `None`. Retries on `ABORTED`.
    async fn claim_txn(&self, nonce: &str) -> Option<String> {
        let access = self.access("claim").await?;
        for _ in 0..MAX_TXN_ATTEMPTS {
            let transaction = self.begin_transaction(&access).await?;
            let existing = match self.get_doc(&access, nonce, &transaction).await {
                Ok(doc) => doc,
                Err(()) => {
                    let _ = self.commit(&access, &release_commit_body(&transaction)).await;
                    return None;
                }
            };
            let Some(doc) = existing else {
                // Absent — nothing to claim; release the transaction.
                let _ = self.commit(&access, &release_commit_body(&transaction)).await;
                return None;
            };
            let live = doc_expires_at(&doc).map(|e| e > Utc::now()).unwrap_or(false);
            let body = delete_commit_body(&transaction, &doc_name(&self.project_id, nonce));
            match self.commit(&access, &body).await {
                Commit::Ok => return if live { doc_token(&doc) } else { None },
                Commit::Aborted => continue,
                Commit::Failed => return None,
            }
        }
        None
    }
}

impl<T: AccessTokenSource> RelayTokenStore for FirestoreRelayStore<T> {
    #[cfg(not(tarpaulin_include))] // network I/O — request/commit shapes are unit-tested
    fn park<'a>(&'a self, nonce: &'a str, token: &'a str) -> RelayStoreFuture<'a, ParkOutcome> {
        Box::pin(async move { self.park_txn(nonce, token).await })
    }

    #[cfg(not(tarpaulin_include))] // network I/O — request/commit shapes are unit-tested
    fn claim<'a>(&'a self, nonce: &'a str) -> RelayStoreFuture<'a, Option<String>> {
        Box::pin(async move { self.claim_txn(nonce).await })
    }
}

// ── HTTP surface ────────────────────────────────────────────────────────────

/// Body of `POST /auth/relay/deposit` — the web app deposits the minted token.
#[derive(Debug, Deserialize)]
pub struct DepositBody {
    /// The CLI-supplied nonce tying this token to the waiting terminal.
    pub nonce: String,
    /// The Firebase ID token to park.
    pub token: String,
}

/// Body of `GET /auth/relay/claim/{nonce}` — the one-time token payload.
#[derive(Debug, Serialize, Deserialize)]
pub struct ClaimPayload {
    /// The relay token.
    pub token: String,
}

/// Mount the relay-token broker endpoints with their own state (the shared
/// [`RelayTokenStore`], no `WebState`), behind a rate limiter. The `deposit`
/// endpoint is unauthenticated and parks an entry per nonce, so a rate cap
/// bounds how fast — and thus how large — the store can grow.
pub fn relay_auth_router(store: Arc<dyn RelayTokenStore>) -> Router {
    Router::new()
        .route("/auth/relay/deposit", post(deposit))
        .route("/auth/relay/claim/{nonce}", get(claim))
        .with_state(store)
        .layer(axum::middleware::from_fn_with_state(
            crate::ratelimit::RateLimit::default(),
            crate::ratelimit::enforce,
        ))
}

/// `POST /auth/relay/deposit` — park the deposited token under its nonce. The
/// first deposit for a nonce wins; a second is a `409 Conflict` (session-fixation
/// guard — see [`RelayTokenStore::park`]).
async fn deposit(
    State(store): State<Arc<dyn RelayTokenStore>>,
    Json(body): Json<DepositBody>,
) -> Response {
    if body.nonce.trim().is_empty() || body.token.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "nonce and token are required").into_response();
    }
    match store.park(&body.nonce, &body.token).await {
        ParkOutcome::Parked => StatusCode::NO_CONTENT.into_response(),
        ParkOutcome::Conflict => {
            (StatusCode::CONFLICT, "a token is already deposited for that nonce").into_response()
        }
        // A transient store error must not masquerade as a conflict — the browser
        // (and the CLI polling claim) should retry, not fail the login silently.
        ParkOutcome::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "the token store is temporarily unavailable; retry",
        )
            .into_response(),
    }
}

/// `GET /auth/relay/claim/{nonce}` — return the parked token once, or `404`.
async fn claim(State(store): State<Arc<dyn RelayTokenStore>>, Path(nonce): Path<String>) -> Response {
    match store.claim(&nonce).await {
        Some(token) => Json(ClaimPayload { token }).into_response(),
        None => (StatusCode::NOT_FOUND, "no token for that nonce").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use chrono::TimeZone;
    use tower::ServiceExt;

    #[tokio::test]
    async fn park_then_claim_is_single_use() {
        let store = InMemoryRelayStore::new();
        assert_eq!(store.park("n1", "fb-token").await, ParkOutcome::Parked);
        assert_eq!(store.claim("n1").await, Some("fb-token".to_string()));
        // A second claim misses (single-use).
        assert_eq!(store.claim("n1").await, None);
        // Unknown nonce misses.
        assert_eq!(store.claim("nope").await, None);
    }

    #[tokio::test]
    async fn first_deposit_wins_a_second_cannot_rebind_the_nonce() {
        let store = InMemoryRelayStore::new();
        // The victim's token is deposited first.
        assert_eq!(store.park("shared-nonce", "victim-token").await, ParkOutcome::Parked);
        // An attacker who learns the nonce cannot overwrite it.
        assert_eq!(store.park("shared-nonce", "attacker-token").await, ParkOutcome::Conflict);
        // The CLI claims the ORIGINAL (victim's) token, not the attacker's.
        assert_eq!(store.claim("shared-nonce").await, Some("victim-token".to_string()));
    }

    #[tokio::test]
    async fn a_freed_nonce_can_be_reused() {
        use std::sync::Mutex as StdMutex;
        use std::time::Instant;

        struct FrozenClock(StdMutex<Instant>);
        impl Clock for FrozenClock {
            fn now(&self) -> Instant {
                *self.0.lock().unwrap()
            }
        }
        let clock = Arc::new(FrozenClock(StdMutex::new(Instant::now())));
        let store = InMemoryRelayStore::with_clock(Duration::from_secs(1), clock.clone());

        assert_eq!(store.park("n", "first").await, ParkOutcome::Parked);
        // After the entry expires, the nonce is free to reuse (abandoned login).
        *clock.0.lock().unwrap() += Duration::from_secs(2);
        assert_eq!(store.park("n", "second").await, ParkOutcome::Parked);
        assert_eq!(store.claim("n").await, Some("second".to_string()));
    }

    #[tokio::test]
    async fn sweep_drops_only_expired_entries() {
        use std::sync::Mutex as StdMutex;
        use std::time::Instant;

        struct FrozenClock(StdMutex<Instant>);
        impl Clock for FrozenClock {
            fn now(&self) -> Instant {
                *self.0.lock().unwrap()
            }
        }
        let clock = Arc::new(FrozenClock(StdMutex::new(Instant::now())));
        let store = InMemoryRelayStore::with_clock(Duration::from_secs(60), clock.clone());

        assert_eq!(store.park("old", "t-old").await, ParkOutcome::Parked);
        *clock.0.lock().unwrap() += Duration::from_secs(30);
        assert_eq!(store.park("fresh", "t-fresh").await, ParkOutcome::Parked);
        *clock.0.lock().unwrap() += Duration::from_secs(40); // old at 70s (expired), fresh at 40s
        store.sweep_expired();
        assert_eq!(store.len(), 1, "only the unexpired entry survives the sweep");
        assert_eq!(store.claim("fresh").await, Some("t-fresh".to_string()));
        assert_eq!(store.claim("old").await, None);
    }

    #[tokio::test]
    async fn second_deposit_for_a_nonce_is_a_conflict() {
        let store: Arc<dyn RelayTokenStore> = Arc::new(InMemoryRelayStore::new());
        let app = relay_auth_router(store);

        let deposit = |token: &str| {
            app.clone().oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/relay/deposit")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"nonce":"n","token":"{token}"}}"#)))
                    .unwrap(),
            )
        };

        assert_eq!(deposit("first").await.unwrap().status(), StatusCode::NO_CONTENT);
        // A racing second deposit under the same nonce is refused.
        assert_eq!(deposit("second").await.unwrap().status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn deposit_then_claim_over_http() {
        let store: Arc<dyn RelayTokenStore> = Arc::new(InMemoryRelayStore::new());
        let app = relay_auth_router(store);

        // Deposit.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/relay/deposit")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"nonce":"abc","token":"fb-xyz"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        // Claim returns the token once.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/auth/relay/claim/abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let payload: ClaimPayload = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.token, "fb-xyz");

        // A second claim is a 404.
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/auth/relay/claim/abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn deposit_rejects_empty_fields() {
        let store: Arc<dyn RelayTokenStore> = Arc::new(InMemoryRelayStore::new());
        let app = relay_auth_router(store);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/relay/deposit")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"nonce":"","token":""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    // ── Firestore backend: offline request/parse SHAPE tests (no network) ────

    #[test]
    fn doc_id_is_a_stable_64char_hex_per_nonce() {
        let a = doc_id("nonce-abc");
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, doc_id("nonce-abc")); // deterministic
        assert_ne!(a, doc_id("nonce-xyz")); // distinct per nonce
    }

    #[test]
    fn doc_name_targets_the_relay_collection_and_hashed_id() {
        let name = doc_name("darkrun-app", "nonce-abc");
        assert_eq!(
            name,
            format!(
                "projects/darkrun-app/databases/(default)/documents/relayBroker/{}",
                doc_id("nonce-abc"),
            )
        );
        // The GET document URL is the REST base joined to that resource name.
        let url = format!("{FIRESTORE_BASE}/{name}");
        assert!(url.starts_with("https://firestore.googleapis.com/v1/projects/darkrun-app/"));
        assert!(url.contains("/documents/relayBroker/"));
    }

    #[test]
    fn begin_transaction_requests_a_read_write_transaction() {
        let b = begin_transaction_body();
        assert!(b["options"]["readWrite"].is_object());
    }

    #[test]
    fn park_commit_upserts_the_token_and_expiry() {
        let name = doc_name("darkrun-app", "n1");
        let body = park_commit_body("tx-123", &name, "fb-token", "2026-07-06T18:05:00Z");
        assert_eq!(body["transaction"], "tx-123");
        let write = &body["writes"][0];
        assert_eq!(write["update"]["name"], name);
        assert_eq!(write["update"]["fields"]["token"]["stringValue"], "fb-token");
        assert_eq!(
            write["update"]["fields"]["expiresAt"]["timestampValue"],
            "2026-07-06T18:05:00Z"
        );
        // Exactly one write.
        assert_eq!(body["writes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn claim_commit_deletes_the_doc() {
        let name = doc_name("darkrun-app", "n1");
        let body = delete_commit_body("tx-456", &name);
        assert_eq!(body["transaction"], "tx-456");
        assert_eq!(body["writes"][0]["delete"], name);
        assert_eq!(body["writes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn release_commit_writes_nothing() {
        let body = release_commit_body("tx-789");
        assert_eq!(body["transaction"], "tx-789");
        assert!(body["writes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn expires_at_encodes_rfc3339_seconds_z() {
        let now = Utc.with_ymd_and_hms(2026, 7, 6, 18, 0, 0).unwrap();
        assert_eq!(expires_at_rfc3339(now, Duration::from_secs(300)), "2026-07-06T18:05:00Z");
        // Zero TTL is the base instant, still second-precision `Z`.
        assert_eq!(expires_at_rfc3339(now, Duration::from_secs(0)), "2026-07-06T18:00:00Z");
    }

    #[test]
    fn doc_fields_parse_back_out_of_a_firestore_document() {
        let doc = serde_json::json!({
            "name": "…/relayBroker/h1",
            "fields": {
                "token": { "stringValue": "fb-token" },
                "expiresAt": { "timestampValue": "2026-07-06T18:05:00Z" },
            }
        });
        assert_eq!(doc_token(&doc).as_deref(), Some("fb-token"));
        assert_eq!(
            doc_expires_at(&doc),
            Some(Utc.with_ymd_and_hms(2026, 7, 6, 18, 5, 0).unwrap())
        );
        // A document missing the fields is tolerated, not panicked on.
        let empty = serde_json::json!({ "fields": {} });
        assert_eq!(doc_token(&empty), None);
        assert_eq!(doc_expires_at(&empty), None);
    }

    #[test]
    fn transaction_id_reads_the_begin_response() {
        let resp = serde_json::json!({ "transaction": "CgYKBBix" });
        assert_eq!(transaction_id(&resp).as_deref(), Some("CgYKBBix"));
        assert_eq!(transaction_id(&serde_json::json!({})), None);
    }
}
