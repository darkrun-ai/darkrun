//! A minimal per-IP fixed-window rate limiter, wired as an axum middleware.
//!
//! Posture: 60 requests/minute per peer
//! IP, applied only in remote mode. A fixed-window counter is enough for a
//! single-reviewer developer service — it bounds floods without the bookkeeping
//! of a true sliding window. Excess requests get `429 Too Many Requests`.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::listen::PeerAddr;
use crate::RouterState;

// `ConnectInfo` is read out of request extensions directly (see the middleware
// below) rather than via an extractor, so the handler signature stays a shape
// axum's `from_fn_with_state` recognizes regardless of whether connect-info was
// installed (it is not, on the in-process test path).

/// Fallback IP used when a request arrives without connect-info (e.g. the
/// in-process `oneshot` test path). Rate limiting in remote mode always runs
/// over the capped listener, which supplies a real peer address.
const UNKNOWN_IP: IpAddr = IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);

const WINDOW: Duration = Duration::from_secs(60);

/// Shared rate-limiter state: per-IP `(window_start, count)`.
#[derive(Clone, Default)]
pub struct RateLimiter {
    inner: Arc<Mutex<HashMap<IpAddr, (Instant, u64)>>>,
}

impl RateLimiter {
    /// Create a fresh limiter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a hit from `ip` and report whether it is within `max` for the
    /// current 60-second window. Rolls the window when it has elapsed.
    pub fn check(&self, ip: IpAddr, max: u64) -> bool {
        let now = Instant::now();
        let mut guard = self.inner.lock().expect("rate limiter poisoned");
        let entry = guard.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) >= WINDOW {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= max
    }
}

/// axum middleware that enforces [`RateLimiter`] in remote mode only.
///
/// The peer address is read from `ConnectInfo<PeerAddr>` when present (it is,
/// for every connection accepted by the capped listener). It is absent only on
/// the in-process test path, where rate limiting is irrelevant.
pub async fn rate_limit_middleware(
    State(state): State<RouterState>,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if state.app.limits.remote {
        let ip = request
            .extensions()
            .get::<ConnectInfo<PeerAddr>>()
            .map(|ConnectInfo(p)| p.0.ip())
            .unwrap_or(UNKNOWN_IP);
        if !state.limiter.check(ip, state.app.limits.rate_limit_per_min) {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
    }
    Ok(next.run(request).await)
}
