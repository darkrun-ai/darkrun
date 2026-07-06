//! Session registry — the cross-instance record of which host holds a live
//! session, and for which owner.
//!
//! The relay's in-memory map ([`Relay`](crate::relay::Relay)) is per Cloud Run
//! instance, so on its own it can't answer "is another instance already hosting
//! this session?" or "who owns it?" once the service scales past one instance.
//! This module moves that authoritative metadata to Firestore, mirroring the
//! relay-token broker ([`relay_broker`](crate::relay_broker)):
//!
//! * one `/sessions/{docId}` document per live **(owner, session)** pair —
//!   `docId = sha256(owner ‖ NUL ‖ session)` — holding
//!   `{ ownerAccountId, hostInstance, expiresAt }`. Keying by (owner, session),
//!   NOT session alone, is a security boundary: session ids derive from
//!   low-entropy run slugs, so a session-only key would let any authenticated
//!   account pre-register (squat) a victim's guessable session id and durably
//!   block its real host. Owner-scoping puts each owner's claim in its own doc
//!   namespace, so a different account simply can't address, block, or read
//!   another owner's session doc.
//! * register-host is a create-if-absent (or take-over-a-stale-doc) read-write
//!   transaction — the load-bearing **single-host-per-(owner, session)**
//!   guarantee;
//! * a heartbeat renews `expiresAt` while the host lives (and re-creates the doc
//!   if native-TTL GC removed it under a still-live host); a crashed/abandoned
//!   host's doc goes stale and becomes takeover-eligible / unreachable;
//! * attach-client authz reads the doc for the ATTACHING CLIENT'S owner: a live
//!   `(client owner, session)` doc authorizes; a missing/expired one is "no
//!   host". A different owner is a different doc, so cross-owner collision is
//!   structurally impossible — there is no "owner mismatch" case to reject.
//!
//! Wall-clock expiry (`expiresAt`) is authoritative on read regardless of
//! Firestore native-TTL GC lag — exactly like the broker.
//!
//! ## Scope of THIS landing (Wave 2, Step 1b)
//!
//! Session **metadata + authz** live in Firestore here; **frame delivery stays
//! in-memory** (the local map in [`Relay`](crate::relay::Relay)). The
//! cross-instance frame bus is Step 1c. So with `max_instances > 1`, a client
//! that lands on a different instance than its host is authorized by Firestore
//! but can't yet exchange frames — the relay stays single-instance-correct until
//! Step 1c. Behind a trait so the relay runs pure-in-memory (no registry) in
//! dev/tests and the Firestore impl wires in for production unchanged.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::push::AccessTokenSource;

/// A session doc's lifetime without a heartbeat. The host renews well within this
/// (see `HEARTBEAT_INTERVAL` in `relay.rs`), so a missed beat or two tolerates a
/// transient blip, but a crashed/abandoned host's doc goes stale — and thus
/// takeover-eligible and unreachable — within this bound.
pub const SESSION_TTL: Duration = Duration::from_secs(90);

/// The future a [`SessionRegistry`] method returns — boxed so the trait stays
/// object-safe (`dyn SessionRegistry`) while the network-backed impl (Firestore)
/// does async I/O, without pulling in an async-trait dependency. Mirrors
/// [`RegistryFuture`](crate::push::RegistryFuture).
pub type SessionRegistryFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The authoritative, cross-instance record of live sessions: which host holds
/// each `(owner, session)` pair. Behind a trait so the relay runs pure-in-memory
/// (registry absent) in dev/tests, and the Firestore-backed impl makes
/// single-host-per-(owner, session) + owner authz correct across Cloud Run
/// instances. Every op is addressed by `(owner, session)` — the doc key — so a
/// different account can never touch another owner's session doc. The methods are
/// async (boxed futures) because the Firestore impl talks to the REST API over
/// HTTP.
pub trait SessionRegistry: Send + Sync {
    /// Claim `(owner, session)`, hosted by `instance`, with expiry `now + TTL`.
    /// Create-if-absent OR take over a STALE doc (expired = a crashed/abandoned
    /// host). Returns `true` if THIS host won the claim; `false` if a LIVE host
    /// already holds it (one host per (owner, session)) or the backend was
    /// unreachable — either way the caller doesn't register and its socket
    /// reconnects to retry.
    fn register_host<'a>(
        &'a self,
        session: &'a str,
        owner: &'a str,
        instance: &'a str,
    ) -> SessionRegistryFuture<'a, bool>;

    /// Renew the `(owner, session)` doc's `expiresAt` — but only while `instance`
    /// still holds it (a takeover after our TTL lapsed must win, so a late
    /// heartbeat never resurrects a doc another host now owns). If the doc is
    /// ABSENT (native-TTL GC removed it while this host is still live), re-create
    /// it via the same create-if-absent CAS so a live host is never orphaned.
    /// Best-effort.
    fn heartbeat<'a>(
        &'a self,
        session: &'a str,
        owner: &'a str,
        instance: &'a str,
    ) -> SessionRegistryFuture<'a, ()>;

    /// Tear the `(owner, session)` doc down (host disconnect) — deleting it only
    /// if `instance` still holds it, so a host never deletes a session a new host
    /// took over. Native TTL + the read-time expiry check are the backstop if this
    /// is lost.
    fn drop_host<'a>(
        &'a self,
        session: &'a str,
        owner: &'a str,
        instance: &'a str,
    ) -> SessionRegistryFuture<'a, ()>;

    /// The `ownerAccountId` of the `(owner, session)` doc if a LIVE host holds it
    /// (`expiresAt` in the future); `None` if that doc is missing or expired.
    /// Sources the attach-client authz for the ATTACHING CLIENT'S own `owner`: a
    /// live doc for its `(owner, session)` is "authorized", a missing/expired one
    /// is "no host". Because the doc key includes the owner, there is no
    /// cross-owner "forbidden" case — a different owner is simply a different (and
    /// here, absent) doc.
    fn lookup_owner<'a>(
        &'a self,
        session: &'a str,
        owner: &'a str,
    ) -> SessionRegistryFuture<'a, Option<String>>;
}

// ── Firestore-backed registry ───────────────────────────────────────────────

/// The Firestore REST API base.
const FIRESTORE_BASE: &str = "https://firestore.googleapis.com/v1";

/// The collection holding live-session records, one document per session.
const SESSIONS_COLLECTION: &str = "sessions";

/// How many times to retry a transaction that commits `ABORTED` (contention).
const MAX_TXN_ATTEMPTS: usize = 5;

/// Stable Firestore document id for an `(owner, session)` pair: the hex-encoded
/// SHA-256 of `owner ‖ NUL ‖ session`. Keying by owner AND session (not session
/// alone) is the security boundary against session-id squatting: a low-entropy,
/// guessable session id maps to a DIFFERENT doc per owner, so an attacker's uid
/// can never address (block/read) the victim's doc. The `NUL` byte separator
/// can't appear in a Firebase uid or a session slug, so no two distinct pairs can
/// collide by concatenation. Hashing also keeps the id fixed-length and path-safe
/// (a slug can contain `/`) — mirrors the broker's per-nonce hashing.
fn doc_id(owner: &str, session: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(owner.as_bytes());
    hasher.update([0u8]); // unambiguous separator — absent from uids and slugs
    hasher.update(session.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The Firestore resource name (no host/`/v1` prefix) for the `(owner, session)`
/// document — the value a write's `update.name` / `delete` takes.
fn doc_name(project_id: &str, owner: &str, session: &str) -> String {
    format!(
        "projects/{project_id}/databases/(default)/documents/{SESSIONS_COLLECTION}/{}",
        doc_id(owner, session),
    )
}

/// The `:beginTransaction` body requesting a read-write transaction.
fn begin_transaction_body() -> serde_json::Value {
    serde_json::json!({ "options": { "readWrite": {} } })
}

/// One upsert write of a session document: `{ ownerAccountId, hostInstance,
/// expiresAt }`, with `expires_at` an RFC3339 `timestampValue`.
fn session_document_write(
    doc_name: &str,
    owner: &str,
    instance: &str,
    expires_at: &str,
) -> serde_json::Value {
    serde_json::json!({
        "update": {
            "name": doc_name,
            "fields": {
                "ownerAccountId": { "stringValue": owner },
                "hostInstance": { "stringValue": instance },
                "expiresAt": { "timestampValue": expires_at },
            }
        }
    })
}

/// The register/heartbeat `:commit` body: within `transaction`, upsert the session
/// document.
fn register_commit_body(
    transaction: &str,
    doc_name: &str,
    owner: &str,
    instance: &str,
    expires_at: &str,
) -> serde_json::Value {
    serde_json::json!({
        "transaction": transaction,
        "writes": [ session_document_write(doc_name, owner, instance, expires_at) ],
    })
}

/// The drop `:commit` body: within `transaction`, delete the session document.
fn delete_commit_body(transaction: &str, doc_name: &str) -> serde_json::Value {
    serde_json::json!({
        "transaction": transaction,
        "writes": [ { "delete": doc_name } ],
    })
}

/// A `:commit` body that writes nothing — releases `transaction` for the paths
/// that decided not to mutate (a live session on register, a not-ours doc on
/// heartbeat/drop).
fn release_commit_body(transaction: &str) -> serde_json::Value {
    serde_json::json!({ "transaction": transaction, "writes": [] })
}

/// Encode the expiry of a session registered at `now` with lifetime `ttl` as a
/// Firestore `timestampValue` (RFC3339, second precision, `Z`).
fn expires_at_rfc3339(now: DateTime<Utc>, ttl: Duration) -> String {
    let expires = now + chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::zero());
    expires.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Parse a Firestore `timestampValue` (RFC3339) into a UTC instant.
fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc))
}

/// A `stringValue` field of a fetched session document, if present.
fn doc_string(doc: &serde_json::Value, key: &str) -> Option<String> {
    Some(doc.get("fields")?.get(key)?.get("stringValue")?.as_str()?.to_string())
}

/// The `ownerAccountId` of a fetched session document, if present.
fn doc_owner(doc: &serde_json::Value) -> Option<String> {
    doc_string(doc, "ownerAccountId")
}

/// The `hostInstance` of a fetched session document, if present.
fn doc_instance(doc: &serde_json::Value) -> Option<String> {
    doc_string(doc, "hostInstance")
}

/// The `expiresAt` instant of a fetched session document, if present + valid.
fn doc_expires_at(doc: &serde_json::Value) -> Option<DateTime<Utc>> {
    let s = doc.get("fields")?.get("expiresAt")?.get("timestampValue")?.as_str()?;
    parse_timestamp(s)
}

/// Whether a fetched session document is live at `now` (`expiresAt` in the
/// future). A doc missing/malformed `expiresAt` reads as NOT live.
fn doc_is_live(doc: &serde_json::Value, now: DateTime<Utc>) -> bool {
    doc_expires_at(doc).map(|e| e > now).unwrap_or(false)
}

/// The transaction id from a `:beginTransaction` response.
fn transaction_id(resp: &serde_json::Value) -> Option<String> {
    Some(resp.get("transaction")?.as_str()?.to_string())
}

/// The outcome of a `:commit` — distinguished so callers can retry on `ABORTED`.
#[cfg(not(tarpaulin_include))]
enum Commit {
    /// The commit succeeded.
    Ok,
    /// `ABORTED` (HTTP 409) — a document read in the transaction changed under
    /// us; the transaction can be retried.
    Aborted,
    /// Any other failure — best-effort, so the caller gives up on this attempt.
    Failed,
}

/// A [`SessionRegistry`] persisted in Firestore via the REST API, so register /
/// heartbeat / drop / lookup work from any Cloud Run instance. register runs as a
/// read-modify-write transaction (single-host-per-session with stale-takeover);
/// wall-clock expiry (`expiresAt`) is authoritative on read regardless of
/// native-TTL GC lag.
pub struct FirestoreSessionRegistry<T: AccessTokenSource> {
    project_id: String,
    tokens: T,
    ttl: Duration,
    http: reqwest::Client,
}

impl<T: AccessTokenSource> FirestoreSessionRegistry<T> {
    /// A registry for `project_id`, authorized by `tokens` (datastore-scoped),
    /// with the default [`SESSION_TTL`].
    pub fn new(project_id: impl Into<String>, tokens: T) -> Self {
        Self {
            project_id: project_id.into(),
            tokens,
            ttl: SESSION_TTL,
            http: reqwest::Client::new(),
        }
    }
}

#[cfg(not(tarpaulin_include))] // network I/O — request/commit shapes are unit-tested
impl<T: AccessTokenSource> FirestoreSessionRegistry<T> {
    /// The `documents` base URL for this database.
    fn documents_base(&self) -> String {
        format!("{FIRESTORE_BASE}/projects/{}/databases/(default)/documents", self.project_id)
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

    /// Open a read-write transaction, returning its id.
    async fn begin_transaction(&self, access: &str) -> Option<String> {
        let url = format!("{}:beginTransaction", self.documents_base());
        let resp = match self
            .http
            .post(url)
            .bearer_auth(access)
            .json(&begin_transaction_body())
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::warn!(status = %r.status(), "Firestore beginTransaction rejected");
                return None;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Firestore beginTransaction failed");
                return None;
            }
        };
        match resp.json::<serde_json::Value>().await {
            Ok(v) => transaction_id(&v),
            Err(e) => {
                tracing::warn!(error = %e, "Firestore beginTransaction parse failed");
                None
            }
        }
    }

    /// Read the `(owner, session)` document within `transaction`. `Ok(None)` =
    /// absent (404), `Ok(Some)` = present, `Err(())` = a read failure (abort this
    /// attempt).
    async fn get_doc(
        &self,
        access: &str,
        owner: &str,
        session: &str,
        transaction: &str,
    ) -> Result<Option<serde_json::Value>, ()> {
        let url = format!("{FIRESTORE_BASE}/{}", doc_name(&self.project_id, owner, session));
        match self
            .http
            .get(url)
            .bearer_auth(access)
            .query(&[("transaction", transaction)])
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r.json::<serde_json::Value>().await.map(Some).map_err(|e| {
                tracing::warn!(error = %e, "Firestore session read parse failed");
            }),
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => Ok(None),
            Ok(r) => {
                tracing::warn!(status = %r.status(), "Firestore session read rejected");
                Err(())
            }
            Err(e) => {
                tracing::warn!(error = %e, "Firestore session read failed");
                Err(())
            }
        }
    }

    /// Commit `body` (an already-built `:commit` payload).
    async fn commit(&self, access: &str, body: &serde_json::Value) -> Commit {
        let url = format!("{}:commit", self.documents_base());
        match self.http.post(url).bearer_auth(access).json(body).send().await {
            Ok(r) if r.status().is_success() => Commit::Ok,
            Ok(r) if r.status() == reqwest::StatusCode::CONFLICT => Commit::Aborted,
            Ok(r) => {
                tracing::warn!(status = %r.status(), "Firestore session commit rejected");
                Commit::Failed
            }
            Err(e) => {
                tracing::warn!(error = %e, "Firestore session commit failed");
                Commit::Failed
            }
        }
    }

    /// register as a read-modify-write transaction: single-host-per-session with
    /// stale-takeover. Absent OR expired → upsert this host's claim (`true`); a
    /// LIVE doc → write nothing and lose (`false`). Retries on `ABORTED`.
    ///
    /// This rests on Firestore Native-mode read-write transactions tracking the
    /// read of the (absent or expired) doc key: two concurrent registers that both
    /// read "not live" serialize — one commits the claim, the other commits
    /// `ABORTED` and retries, then sees the now-live doc and loses. This is the
    /// load-bearing single-host property; a move to Datastore-mode/optimistic
    /// semantics would silently regress it.
    async fn register_txn(&self, session: &str, owner: &str, instance: &str) -> bool {
        let Some(access) = self.access("register_host").await else {
            return false;
        };
        for _ in 0..MAX_TXN_ATTEMPTS {
            let Some(transaction) = self.begin_transaction(&access).await else {
                return false;
            };
            let existing = match self.get_doc(&access, owner, session, &transaction).await {
                Ok(doc) => doc,
                Err(()) => {
                    let _ = self.commit(&access, &release_commit_body(&transaction)).await;
                    return false;
                }
            };
            if existing.as_ref().map(|d| doc_is_live(d, Utc::now())).unwrap_or(false) {
                // A live host already holds this (owner, session) — release + lose.
                let _ = self.commit(&access, &release_commit_body(&transaction)).await;
                return false;
            }
            let expires = expires_at_rfc3339(Utc::now(), self.ttl);
            let body = register_commit_body(
                &transaction,
                &doc_name(&self.project_id, owner, session),
                owner,
                instance,
                &expires,
            );
            match self.commit(&access, &body).await {
                Commit::Ok => return true,
                Commit::Aborted => continue,
                Commit::Failed => return false,
            }
        }
        false // exhausted retries under contention
    }

    /// heartbeat as a read-modify-write transaction on the `(owner, session)` doc:
    /// renew `expiresAt` while `instance` still holds it, OR re-create it if it's
    /// ABSENT (native-TTL GC removed it under a still-live host). A doc now owned
    /// by ANOTHER instance is left untouched — a takeover must still win. Retries
    /// on `ABORTED`, which is exactly what makes the self-heal safe: if a takeover
    /// races the re-create, the create-if-absent read of the (absent) key aborts,
    /// and the retry re-reads the now-live doc as "not ours" and releases.
    async fn heartbeat_txn(&self, session: &str, owner: &str, instance: &str) {
        let Some(access) = self.access("heartbeat").await else {
            return;
        };
        for _ in 0..MAX_TXN_ATTEMPTS {
            let Some(transaction) = self.begin_transaction(&access).await else {
                return;
            };
            let existing = match self.get_doc(&access, owner, session, &transaction).await {
                Ok(doc) => doc,
                Err(()) => {
                    let _ = self.commit(&access, &release_commit_body(&transaction)).await;
                    return;
                }
            };
            // Renew if the doc is ours; re-create if it's absent (self-heal); leave
            // a doc another instance now holds untouched (takeover wins).
            let write = match &existing {
                None => true,
                Some(doc) => doc_instance(doc).as_deref() == Some(instance),
            };
            let body = if write {
                let expires = expires_at_rfc3339(Utc::now(), self.ttl);
                register_commit_body(
                    &transaction,
                    &doc_name(&self.project_id, owner, session),
                    owner,
                    instance,
                    &expires,
                )
            } else {
                release_commit_body(&transaction)
            };
            match self.commit(&access, &body).await {
                Commit::Ok | Commit::Failed => return,
                Commit::Aborted => continue,
            }
        }
    }

    /// drop as a read-modify-write transaction: delete the `(owner, session)` doc
    /// only while `instance` still holds it (never delete a session a new host took
    /// over). Retries on `ABORTED`.
    async fn drop_txn(&self, session: &str, owner: &str, instance: &str) {
        let Some(access) = self.access("drop_host").await else {
            return;
        };
        for _ in 0..MAX_TXN_ATTEMPTS {
            let Some(transaction) = self.begin_transaction(&access).await else {
                return;
            };
            let existing = match self.get_doc(&access, owner, session, &transaction).await {
                Ok(doc) => doc,
                Err(()) => {
                    let _ = self.commit(&access, &release_commit_body(&transaction)).await;
                    return;
                }
            };
            let ours = existing.as_ref().and_then(doc_instance).as_deref() == Some(instance);
            let body = if ours {
                delete_commit_body(&transaction, &doc_name(&self.project_id, owner, session))
            } else {
                release_commit_body(&transaction)
            };
            match self.commit(&access, &body).await {
                Commit::Ok | Commit::Failed => return,
                Commit::Aborted => continue,
            }
        }
    }

    /// lookup the owner of the LIVE `(owner, session)` doc with a plain
    /// (non-transactional) read — the attach-client authz doesn't mutate, so it
    /// needs no transaction. Reads the doc keyed by the ATTACHING CLIENT'S `owner`:
    /// a missing (404) OR expired doc → `None` (the caller maps that to "no host").
    /// A different owner is a different doc key, so this can never surface another
    /// owner's session.
    async fn lookup_txn(&self, session: &str, owner: &str) -> Option<String> {
        let access = self.access("lookup_owner").await?;
        let url = format!("{FIRESTORE_BASE}/{}", doc_name(&self.project_id, owner, session));
        let resp = match self.http.get(url).bearer_auth(&access).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => return None,
            Ok(r) => {
                tracing::warn!(status = %r.status(), "Firestore session lookup rejected");
                return None;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Firestore session lookup failed");
                return None;
            }
        };
        let doc = match resp.json::<serde_json::Value>().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "Firestore session lookup parse failed");
                return None;
            }
        };
        // Wall-clock expiry is authoritative even if native TTL hasn't GC'd yet.
        doc_is_live(&doc, Utc::now()).then(|| doc_owner(&doc)).flatten()
    }
}

impl<T: AccessTokenSource> SessionRegistry for FirestoreSessionRegistry<T> {
    #[cfg(not(tarpaulin_include))] // network I/O — request/commit shapes are unit-tested
    fn register_host<'a>(
        &'a self,
        session: &'a str,
        owner: &'a str,
        instance: &'a str,
    ) -> SessionRegistryFuture<'a, bool> {
        Box::pin(async move { self.register_txn(session, owner, instance).await })
    }

    #[cfg(not(tarpaulin_include))] // network I/O — request/commit shapes are unit-tested
    fn heartbeat<'a>(
        &'a self,
        session: &'a str,
        owner: &'a str,
        instance: &'a str,
    ) -> SessionRegistryFuture<'a, ()> {
        Box::pin(async move { self.heartbeat_txn(session, owner, instance).await })
    }

    #[cfg(not(tarpaulin_include))] // network I/O — request/commit shapes are unit-tested
    fn drop_host<'a>(
        &'a self,
        session: &'a str,
        owner: &'a str,
        instance: &'a str,
    ) -> SessionRegistryFuture<'a, ()> {
        Box::pin(async move { self.drop_txn(session, owner, instance).await })
    }

    #[cfg(not(tarpaulin_include))] // network I/O — request/parse shapes are unit-tested
    fn lookup_owner<'a>(
        &'a self,
        session: &'a str,
        owner: &'a str,
    ) -> SessionRegistryFuture<'a, Option<String>> {
        Box::pin(async move { self.lookup_txn(session, owner).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // ── Firestore backend: offline request/parse SHAPE tests (no network) ────

    #[test]
    fn doc_id_is_a_stable_64char_hex_per_owner_and_session() {
        let a = doc_id("owner-1", "sess-abc");
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, doc_id("owner-1", "sess-abc")); // deterministic
        assert_ne!(a, doc_id("owner-1", "sess-xyz")); // distinct per session
        // Owner-scoped: the SAME session id under a DIFFERENT owner is a DIFFERENT
        // doc — this is the anti-squatting boundary.
        assert_ne!(a, doc_id("owner-2", "sess-abc"));
        // The NUL separator disambiguates the split point: for inputs that don't
        // themselves contain NUL (a Firebase uid / run slug never does), different
        // (owner, session) pairs that would collide under a plain concatenation
        // ("ab"+"c" == "a"+"bc") hash to distinct ids.
        assert_ne!(doc_id("ab", "c"), doc_id("a", "bc"));
    }

    #[test]
    fn doc_name_targets_the_sessions_collection_and_hashed_id() {
        let name = doc_name("darkrun-app", "owner-1", "sess-abc");
        assert_eq!(
            name,
            format!(
                "projects/darkrun-app/databases/(default)/documents/sessions/{}",
                doc_id("owner-1", "sess-abc"),
            )
        );
        // The GET document URL is the REST base joined to that resource name.
        let url = format!("{FIRESTORE_BASE}/{name}");
        assert!(url.starts_with("https://firestore.googleapis.com/v1/projects/darkrun-app/"));
        assert!(url.contains("/documents/sessions/"));
    }

    #[test]
    fn begin_transaction_requests_a_read_write_transaction() {
        let b = begin_transaction_body();
        assert!(b["options"]["readWrite"].is_object());
    }

    #[test]
    fn register_commit_upserts_owner_instance_and_expiry() {
        let name = doc_name("darkrun-app", "acct-owner", "s1");
        let body = register_commit_body(
            "tx-123",
            &name,
            "acct-owner",
            "inst-7",
            "2026-07-06T18:05:00Z",
        );
        assert_eq!(body["transaction"], "tx-123");
        let write = &body["writes"][0];
        assert_eq!(write["update"]["name"], name);
        assert_eq!(write["update"]["fields"]["ownerAccountId"]["stringValue"], "acct-owner");
        assert_eq!(write["update"]["fields"]["hostInstance"]["stringValue"], "inst-7");
        assert_eq!(
            write["update"]["fields"]["expiresAt"]["timestampValue"],
            "2026-07-06T18:05:00Z"
        );
        // Exactly one write.
        assert_eq!(body["writes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn drop_commit_deletes_the_doc() {
        let name = doc_name("darkrun-app", "acct-owner", "s1");
        let body = delete_commit_body("tx-456", &name);
        assert_eq!(body["transaction"], "tx-456");
        assert_eq!(body["writes"][0]["delete"], name);
        assert_eq!(body["writes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn release_commit_writes_nothing() {
        let body = release_commit_body("tx-789");
        assert_eq!(body["transaction"], "tx-789");
        assert!(body["writes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn expires_at_encodes_rfc3339_seconds_z() {
        let now = Utc.with_ymd_and_hms(2026, 7, 6, 18, 0, 0).unwrap();
        assert_eq!(expires_at_rfc3339(now, Duration::from_secs(90)), "2026-07-06T18:01:30Z");
        // Zero TTL is the base instant, still second-precision `Z`.
        assert_eq!(expires_at_rfc3339(now, Duration::from_secs(0)), "2026-07-06T18:00:00Z");
    }

    #[test]
    fn doc_fields_parse_back_out_of_a_firestore_document() {
        let doc = serde_json::json!({
            "name": "…/sessions/h1",
            "fields": {
                "ownerAccountId": { "stringValue": "acct-owner" },
                "hostInstance": { "stringValue": "inst-7" },
                "expiresAt": { "timestampValue": "2026-07-06T18:05:00Z" },
            }
        });
        assert_eq!(doc_owner(&doc).as_deref(), Some("acct-owner"));
        assert_eq!(doc_instance(&doc).as_deref(), Some("inst-7"));
        assert_eq!(
            doc_expires_at(&doc),
            Some(Utc.with_ymd_and_hms(2026, 7, 6, 18, 5, 0).unwrap())
        );
        // A document missing the fields is tolerated, not panicked on.
        let empty = serde_json::json!({ "fields": {} });
        assert_eq!(doc_owner(&empty), None);
        assert_eq!(doc_instance(&empty), None);
        assert_eq!(doc_expires_at(&empty), None);
    }

    #[test]
    fn doc_is_live_compares_expiry_to_now() {
        let doc = serde_json::json!({
            "fields": { "expiresAt": { "timestampValue": "2026-07-06T18:05:00Z" } }
        });
        // Before expiry → live; at/after → dead.
        assert!(doc_is_live(&doc, Utc.with_ymd_and_hms(2026, 7, 6, 18, 4, 59).unwrap()));
        assert!(!doc_is_live(&doc, Utc.with_ymd_and_hms(2026, 7, 6, 18, 5, 0).unwrap()));
        assert!(!doc_is_live(&doc, Utc.with_ymd_and_hms(2026, 7, 6, 18, 6, 0).unwrap()));
        // A doc with no expiry never reads as live.
        assert!(!doc_is_live(&serde_json::json!({ "fields": {} }), Utc::now()));
    }

    #[test]
    fn transaction_id_reads_the_begin_response() {
        let resp = serde_json::json!({ "transaction": "CgYKBBix" });
        assert_eq!(transaction_id(&resp).as_deref(), Some("CgYKBBix"));
        assert_eq!(transaction_id(&serde_json::json!({})), None);
    }
}
