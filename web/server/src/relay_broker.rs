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

    /// Park `token` under `nonce`, replacing any prior entry. Expires `ttl` from
    /// now even if never claimed.
    pub fn park(&self, nonce: impl Into<String>, token: impl Into<String>) {
        let expires_at = self.clock.now() + self.ttl;
        if let Ok(mut map) = self.inner.lock() {
            map.insert(nonce.into(), Entry { token: token.into(), expires_at });
        }
    }

    /// Claim the token parked under `nonce`, evicting it. `None` if unknown,
    /// already claimed, or expired — single-use, so a replay can't resurrect it.
    pub fn claim(&self, nonce: &str) -> Option<String> {
        let now = self.clock.now();
        let mut map = self.inner.lock().ok()?;
        let entry = map.remove(nonce)?;
        (entry.expires_at > now).then_some(entry.token)
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

/// Mount the relay-token broker endpoints with their own state (no `WebState`).
pub fn relay_auth_router(broker: RelayBroker) -> Router {
    Router::new()
        .route("/auth/relay/deposit", post(deposit))
        .route("/auth/relay/claim/{nonce}", get(claim))
        .with_state(broker)
}

/// `POST /auth/relay/deposit` — park the deposited token under its nonce.
async fn deposit(State(broker): State<RelayBroker>, Json(body): Json<DepositBody>) -> Response {
    if body.nonce.trim().is_empty() || body.token.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "nonce and token are required").into_response();
    }
    broker.park(body.nonce, body.token);
    StatusCode::NO_CONTENT.into_response()
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
        broker.park("n1", "fb-token");
        assert_eq!(broker.claim("n1"), Some("fb-token".to_string()));
        // A second claim misses (single-use).
        assert_eq!(broker.claim("n1"), None);
        // Unknown nonce misses.
        assert_eq!(broker.claim("nope"), None);
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
