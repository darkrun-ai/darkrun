//! Firebase Auth sign-in (the wasm side of the login round-trip).
//!
//! The browser mints the token here: [`sign_in`] calls the Firebase JS SDK glue
//! (`js/firebase-login.js`) to sign in with GitHub/GitLab and returns the user's
//! Firebase ID token; [`deposit`] POSTs it to the relay broker under the CLI's
//! nonce, where `darkrun login` claims it. That closes the chain the rest of the
//! stack already implements (broker → engine → relay verifier).

use gloo_net::http::Request;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

// The broker that carries the token to the CLI lives in `darkrun-web` (served at
// the website host). Overridable for dev via the `?web=` query param.
const DEFAULT_WEB_BASE: &str = "https://darkrun.ai";

#[wasm_bindgen(module = "/js/firebase-login.js")]
extern "C" {
    /// Start a full-page redirect sign-in for `provider` ("github" | "gitlab").
    /// The page navigates away; on return, `consumeRedirect` reports the outcome.
    /// Defined in the JS glue.
    #[wasm_bindgen(catch)]
    async fn startSignInRedirect(provider: &str) -> Result<JsValue, JsValue>;

    /// Start a full-page redirect to link `provider` to the signed-in account.
    /// Defined in the JS glue.
    #[wasm_bindgen(catch)]
    async fn startLinkRedirect(provider: &str) -> Result<JsValue, JsValue>;

    /// Consume a pending redirect result on load — resolves to the outcome JSON
    /// `{ mode, idToken, accessToken, provider }`, or "" if there is no pending
    /// redirect. Defined in the JS glue.
    #[wasm_bindgen(catch)]
    async fn consumeRedirect() -> Result<JsValue, JsValue>;
}

/// Start a full-page redirect sign-in for `provider`. On success the page
/// navigates away (this future may not return); `Err` means it failed before
/// navigating. The outcome is picked up by [`consume_redirect`] on the next load.
pub async fn start_sign_in_redirect(provider: &str) -> Result<(), String> {
    startSignInRedirect(provider)
        .await
        .map(|_| ())
        .map_err(|e| js_error(&e))
}

/// Start a full-page redirect to link `provider` to the signed-in account.
pub async fn start_link_redirect(provider: &str) -> Result<(), String> {
    startLinkRedirect(provider)
        .await
        .map(|_| ())
        .map_err(|e| js_error(&e))
}

/// The signed-in identity the standalone dashboard works with: the Firebase ID
/// token (account identity) plus the provider OAuth access token (the key that
/// lists the user's repos through `darkrun-web`'s `/api/repos` proxy).
#[derive(Clone, PartialEq, Deserialize)]
pub struct Session {
    /// The Firebase ID token (the account `uid` after verification).
    #[serde(rename = "idToken")]
    pub id_token: String,
    /// The provider OAuth access token (may be empty if the provider returned none).
    #[serde(rename = "accessToken")]
    pub access_token: String,
    /// The provider key (`github` | `gitlab`).
    pub provider: String,
}

/// Consume a pending redirect result on load. `Ok(None)` means there is no
/// pending redirect (a normal page load); `Ok(Some(session))` means we just
/// returned from a provider and the sign-in / link completed. (The JS also
/// reports a `mode` field; the app treats sign-in and link the same on return,
/// so [`Session`] simply ignores it.)
pub async fn consume_redirect() -> Result<Option<Session>, String> {
    match consumeRedirect().await {
        Ok(v) => {
            let json = v.as_string().unwrap_or_default();
            if json.is_empty() {
                return Ok(None);
            }
            serde_json::from_str::<Session>(&json)
                .map(Some)
                .map_err(|e| format!("couldn't read the sign-in result: {e}"))
        }
        Err(e) => Err(js_error(&e)),
    }
}

/// The two providers a darkrun account can link. Order is the display order.
pub const PROVIDERS: [&str; 2] = ["github", "gitlab"];

/// Human label for a provider key.
pub fn provider_label(provider: &str) -> &'static str {
    if provider == "gitlab" { "GitLab" } else { "GitHub" }
}

/// A signed-in account: one Firebase identity (uid) that may link BOTH GitHub and
/// GitLab. Each linked provider keeps its own OAuth access token (Firebase only
/// vends those at sign-in/link time, so they live for the session). The dashboard
/// lists every linked provider's repos as one combined portfolio.
#[derive(Clone, PartialEq)]
pub struct Account {
    /// One [`Session`] per linked provider, all under the same Firebase uid.
    pub identities: Vec<Session>,
}

impl Account {
    /// A fresh account from the first sign-in.
    pub fn new(first: Session) -> Self {
        Self { identities: vec![first] }
    }

    /// Is `provider` already linked to this account?
    pub fn has(&self, provider: &str) -> bool {
        self.identities.iter().any(|s| s.provider == provider)
    }

    /// The identity (token) for `provider`, if linked.
    pub fn identity_for(&self, provider: &str) -> Option<&Session> {
        self.identities.iter().find(|s| s.provider == provider)
    }

    /// The provider not yet linked (`github`/`gitlab`), if exactly one is missing
    /// — what the "Link …" button offers. `None` once both are linked.
    pub fn missing_provider(&self) -> Option<&'static str> {
        PROVIDERS.into_iter().find(|p| !self.has(p))
    }
}

/// The deposit body posted to the relay broker.
#[derive(Serialize)]
struct Deposit<'a> {
    nonce: &'a str,
    token: &'a str,
}

/// POST the minted token to the relay broker under `nonce`, so the waiting CLI
/// claims it. `web_base` is the website host (the broker lives there).
pub async fn deposit(web_base: &str, nonce: &str, token: &str) -> Result<(), String> {
    let url = format!("{}/auth/relay/deposit", web_base.trim_end_matches('/'));
    let resp = Request::post(&url)
        .json(&Deposit { nonce, token })
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.ok() {
        Ok(())
    } else {
        Err(format!("broker returned {}", resp.status()))
    }
}

/// The website host the broker is served from (`?web=` override, else the
/// default).
pub fn web_base() -> String {
    query_param("web").unwrap_or_else(|| DEFAULT_WEB_BASE.to_string())
}

/// Read a query param from the page URL.
pub fn query_param(key: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    let query = search.trim_start_matches('?');
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v.replace("%3A", ":").replace("%2F", "/").replace("%2f", "/"));
            }
        }
    }
    None
}

/// Render a JS error value to a readable string. Shared with [`crate::register`].
pub(crate) fn js_error(e: &JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::Reflect::get(e, &JsValue::from_str("message")).ok()?.as_string())
        .unwrap_or_else(|| "sign-in failed or was cancelled".to_string())
}
