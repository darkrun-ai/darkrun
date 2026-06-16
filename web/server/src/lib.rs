//! darkrun-web — the server-backed website host.
//!
//! One axum server fronts two things:
//!
//! 1. **OAuth host.** The website performs the OAuth dance for the CLI's
//!    brokered authorization-code flow. The browser hits
//!    `/auth/:provider/start`, the provider calls back to
//!    `/auth/:provider/callback`, the server exchanges the code for a token
//!    using the client secret (server env only), parks it under the CLI's nonce
//!    in a short-lived in-memory [`Broker`], and the CLI claims it once from
//!    `/auth/broker/:nonce`. Client secrets never leave the server.
//!
//! 2. **Static site.** The built Dioxus wasm SPA (`web/site/dist`) is served as
//!    static files with an SPA fallback to `index.html`, so a single process
//!    hosts both the marketing site and the OAuth endpoints.
//!
//! The networking seam is darkrun-vcs's [`HttpTransport`]: production wires the
//! [`ReqwestTransport`]; tests inject a mock so the suite is fully offline.
//!
//! Entry points: [`serve`] (env-configured, production) and [`build_router`]
//! (for in-process `tower::ServiceExt::oneshot` tests).

#![deny(missing_docs)]

mod broker;
mod config;
mod firebase_auth;
mod gcp_auth;
mod oauth_routes;
mod push;
mod relay;
mod relay_broker;
mod state;
mod transport;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{routing::get, Router};
use tower_http::services::{ServeDir, ServeFile};

pub use broker::{Broker, Clock, SystemClock, DEFAULT_TTL};
pub use config::{ProviderCredentials, WebConfig, DEFAULT_WEB_BASE};
pub use oauth_routes::BrokerPayload;
pub use firebase_auth::{FirebaseTokenAuth, FIREBASE_CERTS_URL};
pub use gcp_auth::{ServiceAccount, ServiceAccountTokenSource, FCM_SCOPE};
pub use relay_broker::{relay_auth_router, ClaimPayload, RelayBroker};
pub use push::{
    fan_out, fcm_endpoint, fcm_message, AccessTokenSource, DeviceRegistry, DeviceToken,
    FcmPushSender, InMemoryDeviceRegistry, NoopPushSender, PushSender, StaticTokenSource,
};
pub use relay::{
    device_router, relay_router, AttachError, DevTokenAuth, Frame, HostCmd, HostEvent,
    RegisterDevice, Relay, RelayAuth, RelayState,
};
pub use state::{SharedTransport, WebState};
pub use transport::ReqwestTransport;

/// The default directory the static site is served from (`web/site/dist`),
/// overridable via `DARKRUN_SITE_DIR`.
pub const DEFAULT_SITE_DIR: &str = "web/site/dist";

/// Build the OAuth sub-router (the three `/auth/...` endpoints).
///
/// Public so tests can mount just the OAuth surface without a site directory.
pub fn oauth_router(state: WebState) -> Router {
    Router::new()
        .route("/auth/{provider}/start", get(oauth_routes::start))
        .route("/auth/{provider}/callback", get(oauth_routes::callback))
        .route("/auth/broker/{nonce}", get(oauth_routes::broker_claim))
        .with_state(state)
}

/// Build a [`ServeDir`] for `site_dir` with an SPA fallback to its
/// `index.html`.
///
/// Unknown paths (client-side routes) fall through to `index.html` so the wasm
/// SPA can take over routing. If `index.html` is absent the fallback still
/// resolves to a `404` from `ServeFile`.
fn site_service(site_dir: &Path) -> ServeDir<ServeFile> {
    let index = site_dir.join("index.html");
    ServeDir::new(site_dir).fallback(ServeFile::new(index))
}

/// Build the fully-wired router: OAuth endpoints plus the static site with SPA
/// fallback. The site directory need not exist yet (requests 404 until built).
pub fn build_router(state: WebState, site_dir: impl AsRef<Path>) -> Router {
    let site_dir = site_dir.as_ref();
    oauth_router(state).fallback_service(site_service(site_dir))
}

/// Build the router with the OAuth surface only (no static site).
///
/// Useful when the site is hosted elsewhere, or for OAuth-focused tests.
pub fn build_oauth_only(state: WebState) -> Router {
    oauth_router(state)
}

/// Build the remote-tunnel relay router from the environment, or `None` when no
/// token verifier is configured — in which case the relay endpoints are NOT
/// exposed (safe default).
///
/// Verifier selection:
/// - `DARKRUN_FIREBASE_PROJECT=<id>` → the production [`FirebaseTokenAuth`]: it
///   verifies a Firebase ID token (from `/darkrun:darkrun-login`) and returns the
///   account `uid`. Google's signing certs are fetched up front and refreshed
///   hourly in the background.
/// - else `DARKRUN_RELAY_DEV_AUTH=1` → [`DevTokenAuth`] (token == account id),
///   for local/dev ONLY — never set this in production.
/// - else `None` (relay closed).
pub async fn relay_router_from_env() -> Option<Router> {
    if let Some(project) = std::env::var("DARKRUN_FIREBASE_PROJECT")
        .ok()
        .filter(|p| !p.trim().is_empty())
    {
        let auth = Arc::new(FirebaseTokenAuth::new(project.clone()));
        match auth.refresh_from_google().await {
            Ok(n) => tracing::info!(keys = n, "loaded Firebase signing certs"),
            Err(e) => tracing::warn!(error = %e, "could not load Firebase certs at startup"),
        }
        // Refresh the certs hourly (they rotate); best-effort.
        let refresher = auth.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
            tick.tick().await;
            loop {
                tick.tick().await;
                if let Err(e) = refresher.refresh_from_google().await {
                    tracing::warn!(error = %e, "Firebase cert refresh failed");
                }
            }
        });
        // Wire FCM remote push when service-account credentials are present
        // (GOOGLE_APPLICATION_CREDENTIALS). Absent → push stays disabled; the
        // host's LOCAL OS notification still fires.
        let mut state = RelayState::new(Arc::new(Relay::new()), auth);
        if let Some(tokens) = ServiceAccountTokenSource::from_env() {
            tracing::info!("FCM remote push enabled (service-account credentials loaded)");
            state = state.with_push(
                Arc::new(InMemoryDeviceRegistry::new()),
                Arc::new(FcmPushSender::new(project, tokens)),
            );
        } else {
            tracing::info!(
                "FCM credentials absent — remote push disabled (local notifications still fire)"
            );
        }
        return Some(relay_router(state.clone()).merge(device_router(state)));
    }
    if std::env::var("DARKRUN_RELAY_DEV_AUTH").ok().as_deref() == Some("1") {
        let state = RelayState::new(Arc::new(Relay::new()), Arc::new(DevTokenAuth));
        return Some(relay_router(state.clone()).merge(device_router(state)));
    }
    None
}

/// Resolve the static site directory from `DARKRUN_SITE_DIR`, falling back to
/// [`DEFAULT_SITE_DIR`].
pub fn site_dir_from_env() -> PathBuf {
    std::env::var("DARKRUN_SITE_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SITE_DIR))
}

/// Build the production [`WebState`] from the environment.
///
/// Reads OAuth client credentials and the web base from env, constructs the
/// live [`ReqwestTransport`], and a default-TTL [`Broker`].
pub fn state_from_env() -> std::io::Result<WebState> {
    let config = WebConfig::from_env();
    let transport =
        ReqwestTransport::new().map_err(|e| std::io::Error::other(e.to_string()))?;
    let transport: SharedTransport = Arc::new(transport);
    Ok(WebState::new(config, Broker::new(), transport))
}

/// Start the website host on `addr`.
///
/// Resolves config, transport, and the site directory from the environment,
/// then serves OAuth + the static site until the process stops.
#[cfg(not(tarpaulin_include))] // socket bind + serve loop
pub async fn serve(addr: SocketAddr) -> std::io::Result<()> {
    let state = state_from_env()?;
    let site_dir = site_dir_from_env();
    let mut router = build_router(state, &site_dir);
    // Mount the remote-tunnel relay when a verifier is configured (else the
    // endpoints stay absent — safe default).
    if let Some(relay) = relay_router_from_env().await {
        router = router.merge(relay);
        tracing::info!("relay endpoints mounted (/relay/host, /relay/client)");
    }
    // The relay-token broker carries a browser-minted Firebase token to the CLI
    // (POST /auth/relay/deposit, GET /auth/relay/claim/:nonce).
    router = router.merge(relay_auth_router(RelayBroker::new()));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(
        %addr,
        site_dir = %site_dir.display(),
        "darkrun website host listening"
    );
    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod lib_env_tests {
    use super::*;

    #[test]
    fn site_dir_and_state_resolve_from_env() {
        let _g = LIB_ENV_LOCK.lock().unwrap();
        std::env::set_var("DARKRUN_SITE_DIR", "/tmp/darkrun-site-xyz");
        assert_eq!(site_dir_from_env(), PathBuf::from("/tmp/darkrun-site-xyz"));
        std::env::remove_var("DARKRUN_SITE_DIR");
        // Falls back to the default when unset/blank.
        assert_eq!(site_dir_from_env(), PathBuf::from(DEFAULT_SITE_DIR));
        // state_from_env builds a live state (config + transport + broker).
        assert!(state_from_env().is_ok());
    }

    static LIB_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
