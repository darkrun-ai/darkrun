//! Firebase Auth sign-in (the wasm side of the login round-trip).
//!
//! The browser mints the token here: [`sign_in`] calls the Firebase JS SDK glue
//! (`js/firebase-login.js`) to sign in with GitHub/GitLab and returns the user's
//! Firebase ID token; [`deposit`] POSTs it to the relay broker under the CLI's
//! nonce, where `darkrun login` claims it. That closes the chain the rest of the
//! stack already implements (broker → engine → relay verifier).

use gloo_net::http::Request;
use serde::Serialize;
use wasm_bindgen::prelude::*;

// The broker that carries the token to the CLI lives in `darkrun-web` (served at
// the website host). Overridable for dev via the `?web=` query param.
const DEFAULT_WEB_BASE: &str = "https://darkrun.ai";

#[wasm_bindgen(module = "/js/firebase-login.js")]
extern "C" {
    /// Sign in with `provider` ("github" | "gitlab") and resolve to the Firebase
    /// ID token. Defined in the JS glue.
    #[wasm_bindgen(catch)]
    async fn signInAndGetToken(provider: &str) -> Result<JsValue, JsValue>;
}

/// Sign in with `provider` and return the Firebase ID token.
pub async fn sign_in(provider: &str) -> Result<String, String> {
    match signInAndGetToken(provider).await {
        Ok(v) => v
            .as_string()
            .ok_or_else(|| "Firebase returned no ID token".to_string()),
        Err(e) => Err(js_error(&e)),
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

/// Render a JS error value to a readable string.
fn js_error(e: &JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::Reflect::get(e, &JsValue::from_str("message")).ok()?.as_string())
        .unwrap_or_else(|| "sign-in failed or was cancelled".to_string())
}
