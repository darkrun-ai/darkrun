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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::broker::{Clock, SystemClock, DEFAULT_TTL};

/// A parked relay token and when it stops being claimable.
struct Entry {
    token: String,
    expires_at: Instant,
}

/// In-memory single-use store of relay tokens keyed by nonce. Cheap to clone
/// (clones share the backing store).
#[derive(Clone)]
pub struct RelayBroker {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
    ttl: Duration,
    clock: Arc<dyn Clock>,
}

impl Default for RelayBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl RelayBroker {
    /// A broker with the default TTL and a real clock.
    pub fn new() -> Self {
        Self::with_clock(DEFAULT_TTL, Arc::new(SystemClock))
    }

    /// A broker with an explicit TTL + clock — the test seam.
    pub fn with_clock(ttl: Duration, clock: Arc<dyn Clock>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ttl,
            clock,
        }
    }

    /// Park `token` under `nonce` — the FIRST deposit wins and binds. Returns
    /// `true` if parked, `false` if a live (unexpired) entry already holds the
    /// nonce.
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
    pub fn park(&self, nonce: impl Into<String>, token: impl Into<String>) -> bool {
        let now = self.clock.now();
        let Ok(mut map) = self.inner.lock() else {
            return false;
        };
        let nonce = nonce.into();
        if let Some(existing) = map.get(&nonce) {
            if existing.expires_at > now {
                return false; // a live token already holds this nonce — don't rebind
            }
        }
        map.insert(nonce, Entry { token: token.into(), expires_at: now + self.ttl });
        true
    }

    /// Claim the token parked under `nonce`, evicting it. `None` if unknown,
    /// already claimed, or expired — single-use, so a replay can't resurrect it.
    pub fn claim(&self, nonce: &str) -> Option<String> {
        let now = self.clock.now();
        let mut map = self.inner.lock().ok()?;
        let entry = map.remove(nonce)?;
        (entry.expires_at > now).then_some(entry.token)
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

    /// Whether the broker currently holds no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

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

/// Mount the relay-token broker endpoints with their own state (no `WebState`),
/// behind a rate limiter. The `deposit` endpoint is unauthenticated and parks an
/// entry per nonce, so a rate cap (paired with the TTL sweep) bounds how fast —
/// and thus how large — the broker map can grow.
pub fn relay_auth_router(broker: RelayBroker) -> Router {
    Router::new()
        .route("/auth/relay/deposit", post(deposit))
        .route("/auth/relay/claim/{nonce}", get(claim))
        .with_state(broker)
        .layer(axum::middleware::from_fn_with_state(
            crate::ratelimit::RateLimit::default(),
            crate::ratelimit::enforce,
        ))
}

/// `POST /auth/relay/deposit` — park the deposited token under its nonce. The
/// first deposit for a nonce wins; a second is a `409 Conflict` (session-fixation
/// guard — see [`RelayBroker::park`]).
async fn deposit(State(broker): State<RelayBroker>, Json(body): Json<DepositBody>) -> Response {
    if body.nonce.trim().is_empty() || body.token.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "nonce and token are required").into_response();
    }
    if broker.park(body.nonce, body.token) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::CONFLICT, "a token is already deposited for that nonce").into_response()
    }
}

/// `GET /auth/relay/claim/{nonce}` — return the parked token once, or `404`.
async fn claim(State(broker): State<RelayBroker>, Path(nonce): Path<String>) -> Response {
    match broker.claim(&nonce) {
        Some(token) => Json(ClaimPayload { token }).into_response(),
        None => (StatusCode::NOT_FOUND, "no token for that nonce").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[test]
    fn park_then_claim_is_single_use() {
        let broker = RelayBroker::new();
        assert!(broker.park("n1", "fb-token"));
        assert_eq!(broker.claim("n1"), Some("fb-token".to_string()));
        // A second claim misses (single-use).
        assert_eq!(broker.claim("n1"), None);
        // Unknown nonce misses.
        assert_eq!(broker.claim("nope"), None);
    }

    #[test]
    fn first_deposit_wins_a_second_cannot_rebind_the_nonce() {
        let broker = RelayBroker::new();
        // The victim's token is deposited first.
        assert!(broker.park("shared-nonce", "victim-token"));
        // An attacker who learns the nonce cannot overwrite it.
        assert!(!broker.park("shared-nonce", "attacker-token"));
        // The CLI claims the ORIGINAL (victim's) token, not the attacker's.
        assert_eq!(broker.claim("shared-nonce"), Some("victim-token".to_string()));
    }

    #[test]
    fn a_freed_nonce_can_be_reused() {
        use std::sync::Mutex as StdMutex;
        use std::time::Instant;

        struct FrozenClock(StdMutex<Instant>);
        impl Clock for FrozenClock {
            fn now(&self) -> Instant {
                *self.0.lock().unwrap()
            }
        }
        let clock = Arc::new(FrozenClock(StdMutex::new(Instant::now())));
        let broker = RelayBroker::with_clock(Duration::from_secs(1), clock.clone());

        assert!(broker.park("n", "first"));
        // After the entry expires, the nonce is free to reuse (abandoned login).
        *clock.0.lock().unwrap() += Duration::from_secs(2);
        assert!(broker.park("n", "second"));
        assert_eq!(broker.claim("n"), Some("second".to_string()));
    }

    #[test]
    fn sweep_drops_only_expired_entries() {
        use std::sync::Mutex as StdMutex;
        use std::time::Instant;

        struct FrozenClock(StdMutex<Instant>);
        impl Clock for FrozenClock {
            fn now(&self) -> Instant {
                *self.0.lock().unwrap()
            }
        }
        let clock = Arc::new(FrozenClock(StdMutex::new(Instant::now())));
        let broker = RelayBroker::with_clock(Duration::from_secs(60), clock.clone());

        assert!(broker.park("old", "t-old"));
        *clock.0.lock().unwrap() += Duration::from_secs(30);
        assert!(broker.park("fresh", "t-fresh"));
        *clock.0.lock().unwrap() += Duration::from_secs(40); // old at 70s (expired), fresh at 40s
        broker.sweep_expired();
        assert_eq!(broker.len(), 1, "only the unexpired entry survives the sweep");
        assert_eq!(broker.claim("fresh"), Some("t-fresh".to_string()));
        assert_eq!(broker.claim("old"), None);
    }

    #[tokio::test]
    async fn second_deposit_for_a_nonce_is_a_conflict() {
        let broker = RelayBroker::new();
        let app = relay_auth_router(broker);

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
        let broker = RelayBroker::new();
        let app = relay_auth_router(broker);

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
        let app = relay_auth_router(RelayBroker::new());
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
}
