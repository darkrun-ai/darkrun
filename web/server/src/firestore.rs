//! Firestore-backed device registry — persistence for FCM registrations.
//!
//! The in-memory registry loses every device on a relay restart/redeploy. This
//! impl stores registrations in Firestore (Firebase-native, no extra infra) via
//! the REST API, authorized by a service-account token scoped to `datastore`.
//!
//! Data model: a top-level `devices` collection, one document per FCM token,
//! keyed by the token's SHA-256 (a stable, doc-id-safe id) so register and
//! unregister address the same document directly. Each doc holds
//! `{ account, token, platform }`; an owner's devices are one equality query on
//! `account`.
//!
//! Failures are best-effort + logged, matching the in-memory registry's
//! infallible signatures: a registration that can't reach Firestore just isn't
//! stored (the client re-registers on next launch), and a failed lookup yields
//! no devices (so the push is skipped, never a crash).

use sha2::{Digest, Sha256};

use crate::push::{AccessTokenSource, DeviceRegistry, DeviceToken, RegistryFuture};

/// The Firestore REST API base.
const FIRESTORE_BASE: &str = "https://firestore.googleapis.com/v1";

/// The collection holding device registrations.
const DEVICES_COLLECTION: &str = "devices";

/// Stable Firestore document id for an FCM token: its SHA-256, hex-encoded. FCM
/// tokens can contain characters that aren't valid in a document id; the hash is
/// fixed-length and safe, and lets register/unregister hit the same document.
fn doc_id(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The Firestore document body (`fields` map) for one registration.
fn device_document(account: &str, token: &str, platform: &str) -> serde_json::Value {
    serde_json::json!({
        "fields": {
            "account": { "stringValue": account },
            "token": { "stringValue": token },
            "platform": { "stringValue": platform },
        }
    })
}

/// The `:runQuery` body selecting every device registered to `account`.
fn devices_query(account: &str) -> serde_json::Value {
    serde_json::json!({
        "structuredQuery": {
            "from": [{ "collectionId": DEVICES_COLLECTION }],
            "where": {
                "fieldFilter": {
                    "field": { "fieldPath": "account" },
                    "op": "EQUAL",
                    "value": { "stringValue": account },
                }
            }
        }
    })
}

/// Parse a `:runQuery` response into the devices it returned. Tolerant: skips
/// rows without a `document` (e.g. the empty-result placeholder) and any row
/// missing the token field.
fn parse_devices(resp: &serde_json::Value) -> Vec<DeviceToken> {
    let field = |fields: &serde_json::Value, key: &str| -> Option<String> {
        Some(fields.get(key)?.get("stringValue")?.as_str()?.to_string())
    };
    resp.as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let fields = row.get("document")?.get("fields")?;
            let token = field(fields, "token")?;
            let platform = field(fields, "platform").unwrap_or_default();
            Some(DeviceToken { token, platform })
        })
        .collect()
}

/// A [`DeviceRegistry`] persisted in Firestore via the REST API.
pub struct FirestoreDeviceRegistry<T: AccessTokenSource> {
    project_id: String,
    tokens: T,
    http: reqwest::Client,
}

impl<T: AccessTokenSource> FirestoreDeviceRegistry<T> {
    /// A registry for `project_id`, authorized by `tokens` (datastore-scoped).
    pub fn new(project_id: impl Into<String>, tokens: T) -> Self {
        Self {
            project_id: project_id.into(),
            tokens,
            http: reqwest::Client::new(),
        }
    }

    /// The document URL for `token`'s registration.
    fn doc_url(&self, token: &str) -> String {
        format!(
            "{FIRESTORE_BASE}/projects/{}/databases/(default)/documents/{DEVICES_COLLECTION}/{}",
            self.project_id,
            doc_id(token),
        )
    }

    /// The `:runQuery` URL for this database.
    fn query_url(&self) -> String {
        format!(
            "{FIRESTORE_BASE}/projects/{}/databases/(default)/documents:runQuery",
            self.project_id,
        )
    }

    /// Fetch a datastore-scoped access token, logging + returning `None` on error.
    async fn access(&self, what: &str) -> Option<String> {
        match self.tokens.access_token().await {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::warn!(error = %e, what, "Firestore token unavailable");
                None
            }
        }
    }
}

impl<T: AccessTokenSource> DeviceRegistry for FirestoreDeviceRegistry<T> {
    #[cfg(not(tarpaulin_include))] // network I/O — the request shape is unit-tested
    fn register<'a>(&'a self, account: &'a str, device: DeviceToken) -> RegistryFuture<'a, ()> {
        Box::pin(async move {
            let Some(access) = self.access("register").await else {
                return;
            };
            let body = device_document(account, &device.token, &device.platform);
            match self
                .http
                .patch(self.doc_url(&device.token))
                .bearer_auth(&access)
                .json(&body)
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => {}
                Ok(r) => tracing::warn!(status = %r.status(), "Firestore register rejected"),
                Err(e) => tracing::warn!(error = %e, "Firestore register failed"),
            }
        })
    }

    #[cfg(not(tarpaulin_include))] // network I/O — the request shape is unit-tested
    fn unregister<'a>(&'a self, token: &'a str) -> RegistryFuture<'a, ()> {
        Box::pin(async move {
            let Some(access) = self.access("unregister").await else {
                return;
            };
            match self.http.delete(self.doc_url(token)).bearer_auth(&access).send().await {
                // 404 = already gone; idempotent, so treat as success.
                Ok(r) if r.status().is_success() || r.status() == reqwest::StatusCode::NOT_FOUND => {}
                Ok(r) => tracing::warn!(status = %r.status(), "Firestore unregister rejected"),
                Err(e) => tracing::warn!(error = %e, "Firestore unregister failed"),
            }
        })
    }

    #[cfg(not(tarpaulin_include))] // network I/O — the parse is unit-tested
    fn devices_for<'a>(&'a self, account: &'a str) -> RegistryFuture<'a, Vec<DeviceToken>> {
        Box::pin(async move {
            let Some(access) = self.access("devices_for").await else {
                return Vec::new();
            };
            let resp = match self
                .http
                .post(self.query_url())
                .bearer_auth(&access)
                .json(&devices_query(account))
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => r,
                Ok(r) => {
                    tracing::warn!(status = %r.status(), "Firestore query rejected");
                    return Vec::new();
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Firestore query failed");
                    return Vec::new();
                }
            };
            match resp.json::<serde_json::Value>().await {
                Ok(v) => parse_devices(&v),
                Err(e) => {
                    tracing::warn!(error = %e, "Firestore query parse failed");
                    Vec::new()
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_id_is_a_stable_64char_hex_per_token() {
        let a = doc_id("fcm-token-abc");
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        // Deterministic, and distinct per token.
        assert_eq!(a, doc_id("fcm-token-abc"));
        assert_ne!(a, doc_id("fcm-token-xyz"));
    }

    #[test]
    fn device_document_has_the_firestore_field_shape() {
        let d = device_document("uid-1", "tok-1", "ios");
        assert_eq!(d["fields"]["account"]["stringValue"], "uid-1");
        assert_eq!(d["fields"]["token"]["stringValue"], "tok-1");
        assert_eq!(d["fields"]["platform"]["stringValue"], "ios");
    }

    #[test]
    fn devices_query_filters_by_account_equality() {
        let q = devices_query("uid-1");
        let sq = &q["structuredQuery"];
        assert_eq!(sq["from"][0]["collectionId"], "devices");
        assert_eq!(sq["where"]["fieldFilter"]["field"]["fieldPath"], "account");
        assert_eq!(sq["where"]["fieldFilter"]["op"], "EQUAL");
        assert_eq!(sq["where"]["fieldFilter"]["value"]["stringValue"], "uid-1");
    }

    #[test]
    fn parse_devices_reads_documents_and_skips_placeholders() {
        // A real runQuery response: a leading readTime-only row (no document) when
        // the first match is delayed, then two device documents.
        let resp = serde_json::json!([
            { "readTime": "2026-01-01T00:00:00Z" },
            { "document": { "name": "…/devices/h1", "fields": {
                "account": { "stringValue": "uid-1" },
                "token": { "stringValue": "tok-1" },
                "platform": { "stringValue": "ios" },
            }}},
            { "document": { "name": "…/devices/h2", "fields": {
                "account": { "stringValue": "uid-1" },
                "token": { "stringValue": "tok-2" },
                "platform": { "stringValue": "web" },
            }}},
        ]);
        let devices = parse_devices(&resp);
        assert_eq!(
            devices,
            vec![
                DeviceToken { token: "tok-1".into(), platform: "ios".into() },
                DeviceToken { token: "tok-2".into(), platform: "web".into() },
            ]
        );
    }

    #[test]
    fn parse_devices_tolerates_empty_and_missing_fields() {
        // Empty result (placeholder only) → no devices.
        assert!(parse_devices(&serde_json::json!([{ "readTime": "t" }])).is_empty());
        // A document missing the token field is skipped, not panicked on.
        let resp = serde_json::json!([
            { "document": { "fields": { "platform": { "stringValue": "ios" } } } }
        ]);
        assert!(parse_devices(&resp).is_empty());
    }
}
