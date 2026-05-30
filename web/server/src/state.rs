//! The shared axum router state.
//!
//! Holds the resolved [`WebConfig`], the in-memory [`Broker`], and the injected
//! [`HttpTransport`] used for the server-side token exchange. Cheap to clone:
//! the config is small, the broker shares its backing store, and the transport
//! is an `Arc`.

use std::sync::Arc;

use darkrun_vcs::HttpTransport;

use crate::broker::Broker;
use crate::config::WebConfig;

/// A transport that is safe to share across the axum worker pool.
///
/// The darkrun-vcs [`HttpTransport`] trait carries no `Send + Sync` bound (its
/// mock is single-threaded), so the server requires those bounds at the seam
/// where the trait object is stored in shared state.
pub type SharedTransport = Arc<dyn HttpTransport + Send + Sync>;

/// The composite state every OAuth handler extracts.
#[derive(Clone)]
pub struct WebState {
    /// OAuth client credentials + public base URL.
    pub config: Arc<WebConfig>,
    /// The short-lived, single-use credential broker.
    pub broker: Broker,
    /// The transport used for the server-side `code` → token exchange.
    pub transport: SharedTransport,
}

impl WebState {
    /// Assemble state from its parts.
    pub fn new(config: WebConfig, broker: Broker, transport: SharedTransport) -> Self {
        Self {
            config: Arc::new(config),
            broker,
            transport,
        }
    }
}
