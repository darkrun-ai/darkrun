//! A tiny in-process fixed-window rate limiter for the unauthenticated public
//! endpoints (the relay-token broker's `POST /auth/relay/deposit` above all).
//!
//! Deposit is unauthenticated and parks an entry per nonce; without a cap an
//! attacker could POST distinct nonces fast enough to grow the broker map until
//! OOM. Paired with the broker's TTL sweep (which bounds an entry's lifetime),
//! bounding the request RATE bounds the map SIZE.
//!
//! This is a GLOBAL cap, not per-IP: the relay sits behind a proxy, so trusting a
//! client address would mean parsing (and trusting) `X-Forwarded-For`. A global
//! window is honest and enough to stop the OOM vector; per-IP fairness is a later
//! refinement. The clock is injectable so the window logic is tested without
//! sleeping.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::broker::{Clock, SystemClock};

/// Default budget: at most this many requests per [`DEFAULT_WINDOW`].
pub const DEFAULT_MAX_REQUESTS: u32 = 120;

/// Default window the budget resets over.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(60);

/// The current window's start + how many requests it has admitted.
struct WindowState {
    started: Instant,
    count: u32,
}

/// A shared, cheaply-cloneable fixed-window limiter. Clones share one counter.
#[derive(Clone)]
pub struct RateLimit {
    inner: Arc<Mutex<WindowState>>,
    max: u32,
    window: Duration,
    clock: Arc<dyn Clock>,
}

impl RateLimit {
    /// A limiter admitting `max` requests per `window`, on the real clock.
    pub fn new(max: u32, window: Duration) -> Self {
        Self::with_clock(max, window, Arc::new(SystemClock))
    }

    /// A limiter with an explicit clock — the test seam.
    pub fn with_clock(max: u32, window: Duration, clock: Arc<dyn Clock>) -> Self {
        let started = clock.now();
        Self {
            inner: Arc::new(Mutex::new(WindowState { started, count: 0 })),
            max,
            window,
            clock,
        }
    }

    /// Record a request against the budget. `true` if it's within budget, `false`
    /// if it should be rejected. The window rolls forward once `window` elapses.
    /// Fails OPEN on a poisoned lock — a limiter must never wedge the service.
    pub fn check(&self) -> bool {
        let now = self.clock.now();
        let Ok(mut w) = self.inner.lock() else {
            return true;
        };
        if now.duration_since(w.started) >= self.window {
            w.started = now;
            w.count = 0;
        }
        if w.count >= self.max {
            return false;
        }
        w.count += 1;
        true
    }
}

impl Default for RateLimit {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_REQUESTS, DEFAULT_WINDOW)
    }
}

/// axum middleware enforcing `limiter`: over-budget requests get `429` and never
/// reach the handler. Wire it with [`from_fn_with_state`](axum::middleware::from_fn_with_state).
pub async fn enforce(State(limiter): State<RateLimit>, req: Request, next: Next) -> Response {
    if limiter.check() {
        next.run(req).await
    } else {
        (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    struct FakeClock(StdMutex<Instant>);
    impl FakeClock {
        fn arc() -> Arc<Self> {
            Arc::new(Self(StdMutex::new(Instant::now())))
        }
        fn advance(&self, by: Duration) {
            *self.0.lock().unwrap() += by;
        }
    }
    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            *self.0.lock().unwrap()
        }
    }

    #[test]
    fn admits_up_to_the_budget_then_rejects_until_the_window_rolls() {
        let clock = FakeClock::arc();
        let limiter = RateLimit::with_clock(2, Duration::from_secs(60), clock.clone());

        assert!(limiter.check(), "1st request within budget");
        assert!(limiter.check(), "2nd request within budget");
        assert!(!limiter.check(), "3rd request over budget");

        // Still within the same window → still rejected.
        clock.advance(Duration::from_secs(30));
        assert!(!limiter.check());

        // Window rolls over → budget refills.
        clock.advance(Duration::from_secs(31));
        assert!(limiter.check(), "budget refills after the window");
    }
}
