//! Google service-account OAuth — mints the access token the FCM sender needs.
//!
//! [`FcmPushSender`](crate::push::FcmPushSender) authorizes each `messages:send`
//! with a bearer token. This implements [`AccessTokenSource`] the way Google's
//! own libraries do for a service account: self-sign a short-lived JWT assertion
//! (RS256, with the SA private key), exchange it at the OAuth token endpoint for
//! an access token, and cache that until just before it expires.
//!
//! The service-account JSON is the engine's RUNTIME credential
//! (`GOOGLE_APPLICATION_CREDENTIALS`) — never a CI secret (deploys use keyless
//! WIF). The pure parts — assertion claims, the cache-freshness decision, and
//! the expiry math — are unit-tested offline; only the signing + network
//! exchange need real credentials.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Deserialize;

use crate::push::AccessTokenSource;

/// The OAuth scope an access token must carry to call FCM `messages:send`.
pub const FCM_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

/// The OAuth scope for the Firestore REST API (the persistent device registry).
pub const DATASTORE_SCOPE: &str = "https://www.googleapis.com/auth/datastore";

/// The default OAuth token endpoint (a service-account JSON normally carries its
/// own `token_uri`, but older keys may omit it).
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// Refresh this many seconds BEFORE the stated expiry, to absorb clock skew and
/// the exchange request's own latency.
const EXPIRY_SKEW_SECS: u64 = 60;

/// The assertion's lifetime. Google caps a SA assertion at one hour.
const ASSERTION_TTL_SECS: u64 = 3600;

/// The OAuth `jwt-bearer` grant type for the SA assertion exchange.
const JWT_BEARER_GRANT: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

/// The fields read from a Google service-account JSON key.
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceAccount {
    /// The SA's email — the assertion's `iss`/`sub`.
    pub client_email: String,
    /// The RSA private key (PEM) the assertion is signed with.
    pub private_key: String,
    /// The token-exchange endpoint.
    #[serde(default = "default_token_uri")]
    pub token_uri: String,
    /// The Firebase/GCP project id (informational here; FCM targeting uses it).
    #[serde(default)]
    pub project_id: String,
}

fn default_token_uri() -> String {
    DEFAULT_TOKEN_URI.to_string()
}

impl ServiceAccount {
    /// Parse a service-account key from its JSON text.
    pub fn from_json(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| format!("parsing service-account JSON: {e}"))
    }

    /// Read + parse a service-account key from a file path.
    #[cfg(not(tarpaulin_include))] // filesystem read
    pub fn from_file(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("reading {path}: {e}"))?;
        Self::from_json(&text)
    }

    /// Read the service-account key from `GOOGLE_APPLICATION_CREDENTIALS` (the
    /// engine's runtime credential), or `None` when unset/blank/unreadable.
    /// Returns the parsed account so callers can mint several scoped token
    /// sources (FCM + Firestore) from the one key.
    #[cfg(not(tarpaulin_include))] // env + filesystem
    pub fn from_env() -> Option<Self> {
        let path = std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
            .ok()
            .filter(|p| !p.trim().is_empty())?;
        match Self::from_file(&path) {
            Ok(account) => Some(account),
            Err(e) => {
                tracing::warn!(error = %e, "could not load service-account credentials");
                None
            }
        }
    }
}

/// The token endpoint's success response (the subset we use).
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

/// A cached access token and the unix second it stops being usable (already
/// skew-adjusted, so a fresh token is fetched a bit early).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Cached {
    token: String,
    good_until: u64,
}

/// Build the JWT assertion claims for `account`/`scope` at `now` (unix seconds).
/// Pure, so the claim set is unit-tested without signing.
fn assertion_claims(account: &ServiceAccount, scope: &str, now: u64) -> serde_json::Value {
    serde_json::json!({
        "iss": account.client_email,
        "sub": account.client_email,
        "scope": scope,
        "aud": account.token_uri,
        "iat": now,
        "exp": now + ASSERTION_TTL_SECS,
    })
}

/// The unix second a token obtained at `now` with lifetime `expires_in` should
/// be considered stale (refreshed early by [`EXPIRY_SKEW_SECS`]). Pure.
fn good_until(now: u64, expires_in: u64) -> u64 {
    now + expires_in.saturating_sub(EXPIRY_SKEW_SECS)
}

/// The still-fresh cached token at `now`, if any. Pure, so the cache decision is
/// unit-tested without a clock.
fn fresh_cached(cache: &Option<Cached>, now: u64) -> Option<String> {
    cache
        .as_ref()
        .filter(|c| now < c.good_until)
        .map(|c| c.token.clone())
}

/// Current unix time in seconds (0 before the epoch — never, in practice).
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// An [`AccessTokenSource`] backed by a Google service account: signs + exchanges
/// a JWT assertion, caching the resulting access token until just before expiry.
pub struct ServiceAccountTokenSource {
    account: ServiceAccount,
    scope: String,
    http: reqwest::Client,
    cache: Mutex<Option<Cached>>,
}

impl ServiceAccountTokenSource {
    /// A token source for `account`, scoped to FCM `messages:send` by default.
    pub fn new(account: ServiceAccount) -> Self {
        Self {
            account,
            scope: FCM_SCOPE.to_string(),
            http: reqwest::Client::new(),
            cache: Mutex::new(None),
        }
    }

    /// Override the OAuth scope (e.g. [`DATASTORE_SCOPE`] for the Firestore
    /// registry). Tokens are scope-specific, so each consumer gets its own source.
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = scope.into();
        self
    }

    /// Build from `GOOGLE_APPLICATION_CREDENTIALS` (the path to the SA JSON), or
    /// `None` when unset/blank/unreadable — in which case the caller leaves
    /// remote push disabled (the local OS notification still fires).
    #[cfg(not(tarpaulin_include))] // env + filesystem
    pub fn from_env() -> Option<Self> {
        let path = std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
            .ok()
            .filter(|p| !p.trim().is_empty())?;
        match ServiceAccount::from_file(&path) {
            Ok(account) => Some(Self::new(account)),
            Err(e) => {
                tracing::warn!(error = %e, "could not load service-account credentials");
                None
            }
        }
    }

    /// Sign the assertion for `now`. Fails only if the SA private key won't load.
    fn build_assertion(&self, now: u64) -> Result<String, String> {
        let claims = assertion_claims(&self.account, &self.scope, now);
        let key = EncodingKey::from_rsa_pem(self.account.private_key.as_bytes())
            .map_err(|e| format!("loading SA private key: {e}"))?;
        encode(&Header::new(Algorithm::RS256), &claims, &key)
            .map_err(|e| format!("signing assertion: {e}"))
    }

    /// Exchange a fresh assertion for an access token and cache it.
    #[cfg(not(tarpaulin_include))] // network exchange
    async fn refresh(&self, now: u64) -> Result<String, String> {
        let assertion = self.build_assertion(now)?;
        let resp = self
            .http
            .post(&self.account.token_uri)
            .form(&[("grant_type", JWT_BEARER_GRANT), ("assertion", &assertion)])
            .send()
            .await
            .map_err(|e| format!("token exchange request: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("token endpoint returned {}", resp.status()));
        }
        let token: TokenResponse = resp
            .json()
            .await
            .map_err(|e| format!("parsing token response: {e}"))?;
        let cached = Cached {
            token: token.access_token.clone(),
            good_until: good_until(now, token.expires_in),
        };
        *self.cache.lock().unwrap() = Some(cached);
        Ok(token.access_token)
    }
}

impl AccessTokenSource for ServiceAccountTokenSource {
    #[cfg(not(tarpaulin_include))] // hits the network on a cache miss
    fn access_token(&self) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
        Box::pin(async move {
            let now = now_unix();
            // Serve from cache while the token is still fresh (lock dropped before
            // any await — never held across the network call).
            if let Some(token) = fresh_cached(&self.cache.lock().unwrap(), now) {
                return Ok(token);
            }
            self.refresh(now).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> ServiceAccount {
        ServiceAccount {
            client_email: "fcm@darkrun-app.iam.gserviceaccount.com".into(),
            private_key: "-----BEGIN PRIVATE KEY-----\nstub\n-----END PRIVATE KEY-----".into(),
            token_uri: "https://oauth2.googleapis.com/token".into(),
            project_id: "darkrun-app".into(),
        }
    }

    #[test]
    fn parses_a_service_account_and_defaults_the_token_uri() {
        // A key without `token_uri` falls back to Google's default endpoint.
        let json = r#"{
            "client_email": "x@p.iam.gserviceaccount.com",
            "private_key": "-----BEGIN PRIVATE KEY-----\nk\n-----END PRIVATE KEY-----",
            "project_id": "p"
        }"#;
        let sa = ServiceAccount::from_json(json).unwrap();
        assert_eq!(sa.client_email, "x@p.iam.gserviceaccount.com");
        assert_eq!(sa.token_uri, DEFAULT_TOKEN_URI);
        assert_eq!(sa.project_id, "p");
        // Garbage is a clear error, not a panic.
        assert!(ServiceAccount::from_json("not json").is_err());
    }

    #[test]
    fn assertion_claims_carry_the_scope_issuer_and_hour_long_window() {
        let claims = assertion_claims(&account(), FCM_SCOPE, 1_000);
        assert_eq!(claims["iss"], "fcm@darkrun-app.iam.gserviceaccount.com");
        assert_eq!(claims["sub"], "fcm@darkrun-app.iam.gserviceaccount.com");
        assert_eq!(claims["scope"], FCM_SCOPE);
        assert_eq!(claims["aud"], "https://oauth2.googleapis.com/token");
        assert_eq!(claims["iat"], 1_000);
        assert_eq!(claims["exp"], 1_000 + 3_600);
    }

    #[test]
    fn good_until_refreshes_early_by_the_skew() {
        // 3600s token obtained at t=1000 is treated as good until 1000+3600-60.
        assert_eq!(good_until(1_000, 3_600), 1_000 + 3_600 - 60);
        // A token whose lifetime is under the skew never reads as fresh.
        assert_eq!(good_until(1_000, 30), 1_000);
    }

    #[test]
    fn fresh_cached_honors_the_expiry_window() {
        assert_eq!(fresh_cached(&None, 100), None);
        let cache = Some(Cached { token: "ya29.abc".into(), good_until: 500 });
        assert_eq!(fresh_cached(&cache, 499), Some("ya29.abc".into()));
        assert_eq!(fresh_cached(&cache, 500), None, "stale exactly at the boundary");
        assert_eq!(fresh_cached(&cache, 600), None);
    }
}
