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

    /// Consume a pending redirect result on load — resolves to the outcome JSON
    /// `{ mode, idToken, accessToken, provider }`, or "" if there is no pending
    /// redirect. Defined in the JS glue.
    #[wasm_bindgen(catch)]
    async fn consumeRedirect() -> Result<JsValue, JsValue>;

    /// Resolve the PERSISTED user's Firebase ID token (the "remember me" path),
    /// or "" when nobody is signed in. Awaits the SDK restoring auth from local
    /// storage. Defined in the JS glue.
    #[wasm_bindgen(catch)]
    async fn currentUserIdToken() -> Result<JsValue, JsValue>;
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

/// The signed-in identity the standalone workspace works with: the Firebase ID
/// token (the DURABLE account identity the App-backed `/api/workspace` +
/// `/api/run` calls authenticate with), plus the relay-refresh material the CLI
/// bridge parks so the engine can re-mint the ID token itself.
///
/// The standalone workspace keys off the Firebase identity (`id_token`) alone;
/// the extra fields only matter on the CLI-login path, where [`deposit`] packs
/// them into the relay-token blob the CLI claims.
#[derive(Clone, PartialEq, Deserialize)]
pub struct Session {
    /// The Firebase ID token (the account `uid` after verification).
    #[serde(rename = "idToken")]
    pub id_token: String,
    /// The Firebase refresh token, when the provider issued one — the material
    /// the securetoken endpoint re-mints the ID token from.
    #[serde(rename = "refreshToken", default)]
    pub refresh_token: Option<String>,
    /// The PUBLIC Firebase Web API key the securetoken endpoint keys on.
    #[serde(rename = "apiKey", default)]
    pub api_key: Option<String>,
    /// The ID token's absolute expiry (unix seconds).
    #[serde(rename = "expiresAt", default)]
    pub expires_at: Option<i64>,
    /// When the ID token was issued (unix seconds).
    #[serde(rename = "issuedAt", default)]
    pub issued_at: Option<i64>,
}

impl Session {
    /// Serialize this session into the JSON `RelayToken` blob the broker carries
    /// to the CLI verbatim. Field names match
    /// `darkrun_mcp::relay_token::RelayToken`, so the CLI parses it directly; a
    /// bare/empty optional collapses to `null` (→ `None`), which yields a
    /// legacy-compatible, unrefreshable token when the provider gave no refresh
    /// material.
    fn relay_token_blob(&self) -> Result<String, String> {
        let clean = |o: &Option<String>| o.as_deref().filter(|s| !s.is_empty()).map(str::to_string);
        let value = serde_json::json!({
            "id_token": self.id_token,
            "refresh_token": clean(&self.refresh_token),
            "api_key": clean(&self.api_key),
            "expires_at": self.expires_at.filter(|&n| n > 0),
            "issued_at": self.issued_at.filter(|&n| n > 0),
        });
        serde_json::to_string(&value).map_err(|e| e.to_string())
    }
}

/// Consume a pending redirect result on load. `Ok(None)` means there is no
/// pending redirect (a normal page load); `Ok(Some(session))` means we just
/// returned from a provider and the sign-in completed. Extra fields the JS
/// reports (`mode`, `accessToken`, `provider`) are ignored — the workspace uses
/// only the Firebase ID token.
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

/// Restore the persisted Firebase session on load: `Some(id_token)` when a user
/// is still signed in (Firebase persists auth in browser local storage, so this
/// survives reloads and new tabs without re-login), or `None` when nobody is.
///
/// This is the "remember me" key: the standalone workspace uses this ID token as
/// the bearer for the App-backed `/api/workspace` + `/api/run` calls, so a
/// returning user lands straight in their workspace with no provider re-auth.
pub async fn restore_session() -> Option<String> {
    match currentUserIdToken().await {
        Ok(v) => {
            let token = v.as_string().unwrap_or_default();
            (!token.is_empty()).then_some(token)
        }
        Err(_) => None,
    }
}

/// The deposit body posted to the relay broker.
#[derive(Serialize)]
struct Deposit<'a> {
    nonce: &'a str,
    token: &'a str,
}

/// POST the minted credential to the relay broker under `nonce`, so the waiting
/// CLI claims it. The deposited "token" is the JSON `RelayToken` blob (ID token +
/// refresh material); the broker treats it as an opaque string, so no broker
/// change is needed. `web_base` is the website host (the broker lives there).
pub async fn deposit(web_base: &str, nonce: &str, session: &Session) -> Result<(), String> {
    let blob = session.relay_token_blob()?;
    let url = format!("{}/auth/relay/deposit", web_base.trim_end_matches('/'));
    let resp = Request::post(&url)
        .json(&Deposit { nonce, token: &blob })
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

/// Read a query param from the page URL. Reads `location.search`; the parsing
/// lives in the pure [`query_param_in`] so it is unit-tested off-browser.
pub fn query_param(key: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    query_param_in(&search, key)
}

/// Read `key` from a raw query string (`?a=1&b=2`, leading `?` optional),
/// percent-decoding the `:` and `/` the app's params carry. Returns the first
/// match, or `None`. Pure (no `web_sys`) so the query parsing is unit-tested.
fn query_param_in(search: &str, key: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    //! Native (`#[test]`) coverage of the query-param parser and the sign-in
    //! result (`Session`) descriptor parsing. Both are pure serde/string logic,
    //! so they run under `cargo test -p darkrun-app`.
    use super::*;

    #[test]
    fn query_param_in_reads_a_present_key() {
        assert_eq!(query_param_in("?provider=github&nonce=abc", "provider"), Some("github".into()));
        assert_eq!(query_param_in("?provider=github&nonce=abc", "nonce"), Some("abc".into()));
        // Leading `?` is optional.
        assert_eq!(query_param_in("web=https://x", "web"), Some("https://x".into()));
    }

    #[test]
    fn query_param_in_is_none_for_a_missing_key() {
        assert_eq!(query_param_in("?provider=github", "nonce"), None);
        assert_eq!(query_param_in("", "provider"), None);
    }

    #[test]
    fn query_param_in_percent_decodes_scheme_and_slashes() {
        // A `?web=` override arrives percent-encoded; the base is restored.
        assert_eq!(
            query_param_in("web=https%3A%2F%2Fdarkrun.ai", "web"),
            Some("https://darkrun.ai".into())
        );
        assert_eq!(query_param_in("p=a%2fb", "p"), Some("a/b".into())); // lowercase %2f
    }

    #[test]
    fn session_parses_the_id_token_and_ignores_extra_fields() {
        // The JS glue reports `{ mode, idToken, accessToken, provider }`; the
        // workspace keys off the Firebase ID token alone, so the rest is ignored.
        let json = r#"{"mode":"signIn","idToken":"tok-123","accessToken":"gho_x","provider":"github"}"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.id_token, "tok-123");
    }

    #[test]
    fn session_requires_the_id_token() {
        // Without the load-bearing field, the result is not a usable session.
        assert!(serde_json::from_str::<Session>(r#"{"mode":"signIn"}"#).is_err());
    }
}
