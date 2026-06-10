//! Client-side OAuth tokens for `/browse`.
//!
//! Mirrors the predecessor: the token lives **client-side** (localStorage) and
//! GraphQL calls go straight from the browser to the host, so neither the token
//! nor the browsed data ever flows through darkrun's servers (a smaller threat
//! surface than a backend proxy).
//!
//! The token is obtained with the **same OAuth broker the CLI uses**
//! (`web/server/src/oauth_routes.rs`): the page opens `/auth/<provider>/start`
//! in a popup, the server does the code↔token exchange (the client secret never
//! leaves the server), parks the token under a nonce, and the page claims it once
//! from `/auth/broker/<nonce>` and stores it locally. Read scope is all browse
//! needs; a token only ever unlocks GitHub's GraphQL (which forbids anonymous
//! access) and private/extra-rate-limit reads.

use dioxus::prelude::*;

/// The localStorage key a provider's browse token is stored under.
fn storage_key(provider: &str) -> String {
    format!("darkrun.browse.token.{provider}")
}

/// Read the stored browse token for `provider` (`github` / `gitlab`), if any.
pub async fn stored_token(provider: &str) -> Option<String> {
    let key = storage_key(provider);
    document::eval(&format!("return (localStorage.getItem('{key}') || '');"))
        .join::<String>()
        .await
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Store a browse token for `provider` directly — the predecessor's
/// paste-a-token path, ideal for local testing without the OAuth round-trip.
/// The value is JSON-encoded so a stray quote can't break the snippet.
pub fn store_token(provider: &str, token: &str) {
    let key = storage_key(provider);
    let value = serde_json::to_string(token.trim()).unwrap_or_default();
    let _ = document::eval(&format!(
        "try{{localStorage.setItem('{key}', {value});}}catch(e){{}}"
    ));
}

/// Forget the stored browse token for `provider`.
pub fn clear_token(provider: &str) {
    let key = storage_key(provider);
    let _ = document::eval(&format!("try{{localStorage.removeItem('{key}');}}catch(e){{}}"));
}

/// Run the OAuth dance in a popup and store + return the token, or `None` if the
/// user closed the popup / it timed out. Browser-only.
///
/// The page generates a nonce, opens `/auth/<provider>/start?state=<nonce>`, and
/// polls the one-time `/auth/broker/<nonce>` until the token is parked (the
/// server side of the exchange is the existing CLI broker, unchanged).
pub async fn connect(provider: &str) -> Option<String> {
    let key = storage_key(provider);
    // A single async eval drives the popup + broker poll. It resolves to the
    // token string (or '' on close/timeout). ~5 min of 1s polls.
    let js = format!(
        r#"return await (async () => {{
  const provider = {provider:?};
  const rand = (window.crypto && crypto.randomUUID) ? crypto.randomUUID()
      : (Date.now().toString(36) + Math.random().toString(36).slice(2));
  const nonce = rand.replace(/[^a-zA-Z0-9]/g, '');
  const popup = window.open('/auth/' + provider + '/start?state=' + nonce,
      'darkrun-oauth', 'width=760,height=860');
  if (!popup) return '';
  for (let i = 0; i < 300; i++) {{
    await new Promise(r => setTimeout(r, 1000));
    try {{
      const res = await fetch('/auth/broker/' + nonce, {{ cache: 'no-store' }});
      if (res.ok) {{
        const j = await res.json();
        if (j && j.access_token) {{
          try {{ localStorage.setItem({key:?}, j.access_token); }} catch (e) {{}}
          try {{ popup.close(); }} catch (e) {{}}
          return j.access_token;
        }}
      }}
    }} catch (e) {{}}
    try {{ if (popup.closed) return ''; }} catch (e) {{}}
  }}
  return '';
}})();"#,
        provider = provider,
        key = key,
    );
    document::eval(&js)
        .join::<String>()
        .await
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
