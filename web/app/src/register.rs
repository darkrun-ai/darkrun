//! Device registration for remote push — the client half of the push path.
//!
//! After the operator opts in, the browser mints an FCM token for this device
//! (via the Firebase Messaging JS glue) and POSTs it to the relay's `/devices`
//! endpoint, authorized by the same Firebase token the connection already uses.
//! The relay's [`DeviceRegistry`](../../../web/server) stores it, so a gate on
//! this account pushes a notification to this browser even when the tab is in
//! the background — completing the loop: client registers → Firestore → host
//! gates → relay fan-out → FCM → here.

use gloo_net::http::Request;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::firebase::js_error;

#[wasm_bindgen(module = "/js/firebase-messaging.js")]
extern "C" {
    /// Request notification permission and resolve to this browser's FCM token.
    /// Defined in the JS glue; rejects on unsupported/denied.
    #[wasm_bindgen(catch)]
    async fn requestPushToken() -> Result<JsValue, JsValue>;
}

/// The `/devices` registration body (the owning account comes from the bearer
/// token, never the body).
#[derive(Serialize)]
struct RegisterBody<'a> {
    token: &'a str,
    platform: &'a str,
}

/// Opt this browser into remote push: mint an FCM token and register it for the
/// account behind `id_token` (the Firebase token the relay connection carries).
/// `web_base` is the relay host, where `/devices` is served.
pub async fn enable_push(web_base: &str, id_token: &str) -> Result<(), String> {
    let fcm_token = match requestPushToken().await {
        Ok(v) => v
            .as_string()
            .ok_or_else(|| "Firebase returned no FCM token".to_string())?,
        Err(e) => return Err(js_error(&e)),
    };
    let url = format!("{}/devices", web_base.trim_end_matches('/'));
    let resp = Request::post(&url)
        .header("Authorization", &format!("Bearer {id_token}"))
        .json(&RegisterBody { token: &fcm_token, platform: "web" })
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.ok() {
        Ok(())
    } else {
        Err(format!("registration failed ({})", resp.status()))
    }
}
