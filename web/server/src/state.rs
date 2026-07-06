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
use crate::firebase_auth::FirebaseTokenAuth;
use crate::github_app::GitHubApp;

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
    /// The darkrun GitHub App, when configured (`GITHUB_APP_ID` /
    /// `GITHUB_APP_PRIVATE_KEY`). Drives the App-backed workspace endpoints;
    /// `None` disables them (they answer with a clear "not configured" error).
    pub github_app: Option<Arc<GitHubApp>>,
    /// The Firebase ID-token verifier, when configured. Authenticates the
    /// workspace endpoints and yields the caller's GitHub identity. `None`
    /// disables them.
    pub firebase_auth: Option<Arc<FirebaseTokenAuth>>,
}

impl WebState {
    /// Assemble state from its parts. The App-backed workspace surface stays
    /// disabled until [`with_app_auth`](Self::with_app_auth) supplies it.
    pub fn new(config: WebConfig, broker: Broker, transport: SharedTransport) -> Self {
        Self {
            config: Arc::new(config),
            broker,
            transport,
            github_app: None,
            firebase_auth: None,
        }
    }

    /// Attach the GitHub App + Firebase verifier that power the persistent-login
    /// workspace endpoints (`GET /api/workspace`, `GET /api/run`). Either being
    /// `None` leaves the surface disabled.
    pub fn with_app_auth(
        mut self,
        github_app: Option<Arc<GitHubApp>>,
        firebase_auth: Option<Arc<FirebaseTokenAuth>>,
    ) -> Self {
        self.github_app = github_app;
        self.firebase_auth = firebase_auth;
        self
    }
}
