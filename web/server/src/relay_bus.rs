//! Cross-instance frame bus — carries relay frames between Cloud Run instances so
//! a HOST on instance A and a CLIENT on instance B can exchange frames.
//!
//! The relay's in-memory map ([`Relay`](crate::relay::Relay)) is per instance, so
//! on its own it can only shuttle frames between a host and a client that landed
//! on the SAME instance (the common case at this scale — the L1 fast path, 0 bus
//! hops). When Step 1b's Firestore registry authorizes a client whose host is on
//! ANOTHER instance, this bus is the path their frames take.
//!
//! It is NOT a new wire protocol to the client: it carries the SAME routing
//! envelope ([`HostCmd::To`](darkrun_api::tunnel::HostCmd) /
//! [`HostEvent`](darkrun_api::tunnel::HostEvent)) that an in-process frame uses,
//! and a delivered bus frame re-enters the very same local delivery methods
//! (`deliver_bus_frame` → `to_client` / host `host_tx`), so bus and in-process
//! frames share one delivery path.
//!
//! ## Two backends behind one [`FrameBus`] trait
//!
//! * [`NoopFrameBus`] — the default for dev/tests/single-instance: it carries
//!   nothing. The relay models "no bus" as `None` (see [`Relay`]), so a
//!   single-instance deploy behaves EXACTLY as before this landing.
//! * [`PubSubFrameBus`] — publishes each frame to a Google Pub/Sub topic
//!   (base64 data + `to_instance`/`session` attributes + an `ordering_key` that
//!   is the `(owner, session)` COMPOSITE so a snapshot precedes its updates
//!   WITHOUT coupling head-of-line ordering across owners that share a slug), and
//!   pulls from a per-instance subscription (created at startup with an
//!   `expiration_policy` TTL so it self-deletes if the instance dies, and
//!   message-ordering on). Every instance's subscription receives every published
//!   frame, so the pull loop DROPS any frame whose `to_instance` isn't ours and
//!   dispatches the rest.
//!
//! ## At-least-once (Step 1d follow-up)
//!
//! Pub/Sub is at-least-once, so a frame can be DELIVERED MORE THAN ONCE or (across
//! ordering-key boundaries) reordered. The `(owner, session)` composite ordering
//! key keeps per-session order (snapshot-before-update holds), but the host still
//! needs command dedupe
//! for at-least-once safety — see the Step 1d TODO in `darkrun-tunnel`'s
//! `handle_client_frame`. This landing does NOT implement that dedupe.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use darkrun_api::tunnel::HostEvent;

use crate::push::AccessTokenSource;
use crate::relay::Relay;

/// One relay frame carried across instances by the [`FrameBus`]. The SAME routing
/// envelope in-process delivery uses — a delivered bus frame re-enters the local
/// delivery path unchanged, so this is not a second wire protocol.
///
/// Every frame carries the authenticated `owner` of its `(owner, session)` pair.
/// Session ids derive from low-entropy run slugs, so two accounts can hold the
/// SAME slug in separate Firestore docs (see `relay_registry`). The receiving
/// instance ENFORCES this owner at delivery — a frame is dropped unless it matches
/// the local host's / remote-client's owner — so a colliding slug across owners
/// can never be delivered to the wrong owner. This is the load-bearing
/// cross-owner isolation guarantee, matching the registry's owner-scoped keying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "dir", rename_all = "snake_case")]
pub enum BusFrame {
    /// Host→client: deliver `data` to `client`, whose sink lives on the target
    /// instance. (`HostCmd::To` crossing instances.)
    ToClient {
        /// The authenticated owner of the session — enforced at delivery.
        owner: String,
        /// The destination client id.
        client: u64,
        /// The raw text frame to deliver.
        data: String,
    },
    /// Client→host: a client lifecycle/frame event for the host on the target
    /// instance, tagged with the client's origin instance so the host can build
    /// its client→instance routing table. (`HostEvent` crossing instances.)
    ToHost {
        /// The authenticated owner of the session — enforced at delivery.
        owner: String,
        /// The instance the client's sink lives on (for the host's routing table).
        from_instance: String,
        /// The client event to deliver to the host.
        event: HostEvent,
    },
}

impl BusFrame {
    /// A host→client frame owned by `owner`, delivering `data` to `client`.
    pub fn to_client(owner: &str, client: u64, data: impl Into<String>) -> Self {
        BusFrame::ToClient { owner: owner.to_string(), client, data: data.into() }
    }

    /// A client→host frame owned by `owner`, from `from_instance`, carrying `event`.
    pub fn to_host(owner: &str, from_instance: &str, event: HostEvent) -> Self {
        BusFrame::ToHost {
            owner: owner.to_string(),
            from_instance: from_instance.to_string(),
            event,
        }
    }

    /// The authenticated owner this frame is scoped to. Delivery drops a frame
    /// whose owner doesn't match the local host's / remote-client's owner, and the
    /// publish path folds it into the composite `(owner, session)` ordering key.
    pub fn owner(&self) -> &str {
        match self {
            BusFrame::ToClient { owner, .. } => owner,
            BusFrame::ToHost { owner, .. } => owner,
        }
    }
}

/// The future a [`FrameBus`] method returns — boxed so the trait stays
/// object-safe (`dyn FrameBus`) while the network-backed impl (Pub/Sub) does
/// async I/O, without an async-trait dependency. Mirrors
/// [`SessionRegistryFuture`](crate::relay_registry::SessionRegistryFuture).
pub type FrameBusFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Publishes a relay frame to a specific instance. Behind a trait so the relay
/// runs pure-in-memory (no bus) in dev/tests and the Pub/Sub impl wires in for
/// production unchanged. `publish` is async (a boxed future) because the Pub/Sub
/// impl talks to the REST API over HTTP.
pub trait FrameBus: Send + Sync {
    /// Publish `frame` to `to_instance` for `session`. Best-effort: a failed
    /// publish is logged, not fatal (the client reconnects and retries).
    fn publish<'a>(
        &'a self,
        to_instance: &'a str,
        session: &'a str,
        frame: BusFrame,
    ) -> FrameBusFuture<'a, ()>;
}

/// A bus that carries nothing — the single-instance/dev/test default. Wiring the
/// relay with NO bus (`None`) is the intended single-instance configuration; this
/// exists for tests and as the trait's trivial case.
pub struct NoopFrameBus;

impl FrameBus for NoopFrameBus {
    fn publish<'a>(
        &'a self,
        _to_instance: &'a str,
        _session: &'a str,
        _frame: BusFrame,
    ) -> FrameBusFuture<'a, ()> {
        Box::pin(async {})
    }
}

// ── Pub/Sub-backed bus ───────────────────────────────────────────────────────

/// The Pub/Sub REST API base.
const PUBSUB_BASE: &str = "https://pubsub.googleapis.com/v1";

/// The ack deadline for a pulled frame — frames dispatch synchronously (local
/// mpsc sends), so a short deadline is plenty.
const ACK_DEADLINE_SECS: u64 = 10;

/// A per-instance subscription's lifetime with no activity — Pub/Sub's minimum
/// (24h). A crashed instance's subscription self-deletes within this, so dead
/// instances don't leave subscriptions accumulating on the topic.
const SUBSCRIPTION_TTL: &str = "86400s";

/// How many frames to pull per request.
const PULL_MAX_MESSAGES: u64 = 100;

/// Pub/Sub rejects a message whose total size exceeds 10 MiB. Relay frames are
/// normally KB-scale (review protocol JSON), so a frame whose base64 body
/// approaches that bound is anomalous — the guard drops it with a warning rather
/// than letting the publish 400 and lose the frame silently. Set safely under the
/// hard 10 MiB so attributes + envelope overhead still fit.
const MAX_PUBLISH_BYTES: usize = 9 * 1024 * 1024;

/// The base64-encoded length of `frame`'s body — what the `data` field costs on
/// the wire. STANDARD base64 encodes every 3 bytes to 4 chars (with padding).
/// Pure, so the size guard is unit-tested without publishing.
fn encoded_body_len(frame: &BusFrame) -> usize {
    let bytes = serde_json::to_vec(frame).unwrap_or_default();
    bytes.len().div_ceil(3) * 4
}

/// Whether `frame`'s encoded body is within the safe publish bound
/// ([`MAX_PUBLISH_BYTES`]). Pure.
fn within_publish_limit(frame: &BusFrame) -> bool {
    encoded_body_len(frame) <= MAX_PUBLISH_BYTES
}

/// The subscription id this instance pulls from — derived from the instance id so
/// each instance has its own, and it self-cleans via the expiration policy.
fn subscription_id(instance_id: &str) -> String {
    format!("relay-frames-{instance_id}")
}

/// The full topic resource name (`projects/{p}/topics/{t}`).
fn topic_resource(project_id: &str, topic: &str) -> String {
    format!("projects/{project_id}/topics/{topic}")
}

/// The full subscription resource name for this instance.
fn subscription_resource(project_id: &str, instance_id: &str) -> String {
    format!("projects/{project_id}/subscriptions/{}", subscription_id(instance_id))
}

/// The `:publish` request body for one frame: the frame JSON as base64 `data`,
/// `to_instance` + `session` attributes, and an `orderingKey` that is the
/// `(owner, session)` COMPOSITE (per-`(owner, session)` order so a snapshot
/// precedes its updates, WITHOUT coupling head-of-line ordering across two owners
/// that hold the same low-entropy slug). The `session` attribute stays the raw
/// slug because delivery re-keys the local maps by it; the frame's own `owner`
/// (carried in `data`) is what delivery ENFORCES. Pure, so the wire shape is
/// unit-tested without publishing.
fn publish_body(to_instance: &str, session: &str, frame: &BusFrame) -> serde_json::Value {
    let bytes = serde_json::to_vec(frame).unwrap_or_default();
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let ordering_key = crate::relay_registry::doc_id(frame.owner(), session);
    serde_json::json!({
        "messages": [ {
            "data": data,
            "attributes": { "to_instance": to_instance, "session": session },
            "orderingKey": ordering_key,
        } ]
    })
}

/// The create-subscription (`PUT projects/{p}/subscriptions/{s}`) request body:
/// bound to `topic`, with the expiration-policy TTL (self-delete if the instance
/// dies) and message ordering ON (snapshot-before-update). Pure.
fn subscription_body(topic_resource: &str) -> serde_json::Value {
    serde_json::json!({
        "topic": topic_resource,
        "ackDeadlineSeconds": ACK_DEADLINE_SECS,
        "expirationPolicy": { "ttl": SUBSCRIPTION_TTL },
        "enableMessageOrdering": true,
    })
}

/// The `to_instance` attribute of a pulled Pub/Sub message.
fn message_to_instance(message: &serde_json::Value) -> Option<&str> {
    message.get("attributes")?.get("to_instance")?.as_str()
}

/// The `session` attribute of a pulled Pub/Sub message.
fn message_session(message: &serde_json::Value) -> Option<String> {
    Some(message.get("attributes")?.get("session")?.as_str()?.to_string())
}

/// Decode a pulled message's base64 `data` into a [`BusFrame`].
fn decode_frame(message: &serde_json::Value) -> Option<BusFrame> {
    let data = message.get("data")?.as_str()?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Decide what to do with one pulled frame's inner `message`: dispatch it locally
/// ONLY if its `to_instance` matches THIS instance (every instance's subscription
/// receives every published frame over the shared topic, so frames addressed
/// elsewhere are DROPPED). Returns `(session, frame)` to dispatch, else `None`.
/// Pure — the drop/dispatch filter is unit-tested. (The caller acks EVERY pulled
/// message regardless, to clear it from this instance's subscription.)
fn plan_delivery(message: &serde_json::Value, instance_id: &str) -> Option<(String, BusFrame)> {
    if message_to_instance(message) != Some(instance_id) {
        return None; // addressed to another instance — drop
    }
    let session = message_session(message)?;
    let frame = decode_frame(message)?;
    Some((session, frame))
}

/// A [`FrameBus`] over Google Pub/Sub REST: publishes frames to a topic and pulls
/// this instance's own subscription, dispatching frames addressed here into the
/// relay's local delivery path. Authorized by a Pub/Sub-scoped
/// [`AccessTokenSource`].
pub struct PubSubFrameBus<T: AccessTokenSource> {
    project_id: String,
    topic: String,
    instance_id: String,
    tokens: T,
    http: reqwest::Client,
}

impl<T: AccessTokenSource> PubSubFrameBus<T> {
    /// A bus for `project_id`/`topic`, identified by `instance_id`, authorized by
    /// `tokens` (Pub/Sub-scoped).
    pub fn new(
        project_id: impl Into<String>,
        topic: impl Into<String>,
        instance_id: impl Into<String>,
        tokens: T,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            topic: topic.into(),
            instance_id: instance_id.into(),
            tokens,
            http: reqwest::Client::new(),
        }
    }
}

#[cfg(not(tarpaulin_include))] // network I/O — request/parse shapes are unit-tested
impl<T: AccessTokenSource + 'static> PubSubFrameBus<T> {
    /// How many times to try subscription setup before giving up (bus stays dark).
    const SETUP_MAX_ATTEMPTS: u32 = 6;
    /// The first setup-retry backoff; doubles each attempt (2, 4, 8, … seconds).
    const SETUP_BASE_BACKOFF_SECS: u64 = 2;
    /// The cap on the setup-retry backoff, so it never grows unbounded.
    const SETUP_MAX_BACKOFF_SECS: u64 = 60;

    /// Fetch a Pub/Sub-scoped access token, logging + returning `None` on error.
    async fn access(&self, what: &str) -> Option<String> {
        match self.tokens.access_token().await {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::warn!(error = %e, what, "Pub/Sub token unavailable");
                None
            }
        }
    }

    /// Publish `frame` to `to_instance` for `session`. Best-effort.
    async fn publish_frame(&self, to_instance: &str, session: &str, frame: &BusFrame) {
        // Guard: drop an anomalously large frame rather than 400ing on the 10 MiB
        // Pub/Sub limit and losing it silently (WS frames are normally KB-scale).
        if !within_publish_limit(frame) {
            tracing::warn!(
                encoded_len = encoded_body_len(frame),
                "Pub/Sub frame over the size guard — dropping (anomalous)"
            );
            return;
        }
        let Some(access) = self.access("publish").await else {
            return;
        };
        let url = format!("{PUBSUB_BASE}/{}:publish", topic_resource(&self.project_id, &self.topic));
        let body = publish_body(to_instance, session, frame);
        match self.http.post(url).bearer_auth(&access).json(&body).send().await {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => tracing::warn!(status = %r.status(), "Pub/Sub publish rejected"),
            Err(e) => tracing::warn!(error = %e, "Pub/Sub publish failed"),
        }
    }

    /// Create this instance's subscription (idempotent: a pre-existing one is a
    /// 409 we treat as success). Returns whether the subscription is usable.
    async fn ensure_subscription(&self, access: &str) -> bool {
        let url = format!("{PUBSUB_BASE}/{}", subscription_resource(&self.project_id, &self.instance_id));
        let body = subscription_body(&topic_resource(&self.project_id, &self.topic));
        match self.http.put(url).bearer_auth(access).json(&body).send().await {
            Ok(r) if r.status().is_success() => true,
            Ok(r) if r.status() == reqwest::StatusCode::CONFLICT => true, // already exists
            Ok(r) => {
                tracing::warn!(status = %r.status(), "Pub/Sub subscription create rejected");
                false
            }
            Err(e) => {
                tracing::warn!(error = %e, "Pub/Sub subscription create failed");
                false
            }
        }
    }

    /// Pull a batch of frames from this instance's subscription (blocking pull).
    /// `None` on a request error (the loop backs off and retries).
    async fn pull(&self, access: &str) -> Option<Vec<serde_json::Value>> {
        let url = format!("{PUBSUB_BASE}/{}:pull", subscription_resource(&self.project_id, &self.instance_id));
        let body = serde_json::json!({ "maxMessages": PULL_MAX_MESSAGES });
        let resp = match self.http.post(url).bearer_auth(access).json(&body).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::warn!(status = %r.status(), "Pub/Sub pull rejected");
                return None;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Pub/Sub pull failed");
                return None;
            }
        };
        let value = resp.json::<serde_json::Value>().await.ok()?;
        Some(
            value
                .get("receivedMessages")
                .and_then(|m| m.as_array())
                .cloned()
                .unwrap_or_default(),
        )
    }

    /// Acknowledge pulled frames so they don't redeliver.
    async fn acknowledge(&self, access: &str, ack_ids: &[String]) {
        if ack_ids.is_empty() {
            return;
        }
        let url = format!("{PUBSUB_BASE}/{}:acknowledge", subscription_resource(&self.project_id, &self.instance_id));
        let body = serde_json::json!({ "ackIds": ack_ids });
        if let Err(e) = self.http.post(url).bearer_auth(access).json(&body).send().await {
            tracing::warn!(error = %e, "Pub/Sub acknowledge failed");
        }
    }

    /// Fetch a token + create the subscription, retrying with bounded exponential
    /// backoff so a transient error or a JUST-APPLIED IAM grant (which can take a
    /// minute to propagate) self-heals without a process restart. Capped at
    /// [`SETUP_MAX_ATTEMPTS`](Self::SETUP_MAX_ATTEMPTS) so a hard, persistent
    /// failure (e.g. a real 403) gives up instead of spinning forever. Returns
    /// whether the subscription is usable.
    async fn setup_subscription_with_retry(&self) -> bool {
        for attempt in 0..Self::SETUP_MAX_ATTEMPTS {
            if let Some(access) = self.access("subscribe").await {
                if self.ensure_subscription(&access).await {
                    return true;
                }
            }
            // Back off before the next attempt (no wait after the final one).
            if attempt + 1 < Self::SETUP_MAX_ATTEMPTS {
                let backoff = (Self::SETUP_BASE_BACKOFF_SECS << attempt)
                    .min(Self::SETUP_MAX_BACKOFF_SECS);
                tracing::warn!(
                    attempt = attempt + 1,
                    backoff_secs = backoff,
                    "frame bus: subscription setup attempt failed; retrying"
                );
                tokio::time::sleep(Duration::from_secs(backoff)).await;
            }
        }
        false
    }

    /// Create the subscription, then pull-loop forever, dispatching each frame
    /// addressed to this instance into `relay`'s local delivery path. Runs for the
    /// process's lifetime (spawned at startup).
    async fn run(self: Arc<Self>, relay: Arc<Relay>) {
        // Bounded retry so a transient error or a freshly-applied IAM grant
        // self-heals; a hard failure disables the bus after the attempts are spent.
        if !self.setup_subscription_with_retry().await {
            tracing::warn!(
                "frame bus: subscription setup failed after retries; cross-instance frames disabled"
            );
            return;
        }
        tracing::info!(instance = %self.instance_id, "frame bus subscriber running");
        loop {
            // Re-fetch the (cached) token each pass so a long-lived loop refreshes.
            let Some(access) = self.access("pull").await else {
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            };
            match self.pull(&access).await {
                Some(messages) if !messages.is_empty() => {
                    let mut ack_ids = Vec::with_capacity(messages.len());
                    for received in &messages {
                        if let Some(ack) = received.get("ackId").and_then(|v| v.as_str()) {
                            ack_ids.push(ack.to_string());
                        }
                        if let Some(message) = received.get("message") {
                            if let Some((session, frame)) = plan_delivery(message, &self.instance_id) {
                                relay.deliver_bus_frame(&session, frame);
                            }
                        }
                    }
                    self.acknowledge(&access, &ack_ids).await;
                }
                // Empty pull (no frames) or a transient error — brief backoff.
                _ => tokio::time::sleep(Duration::from_millis(500)).await,
            }
        }
    }

    /// Spawn the subscriber [`run`](Self::run) loop for `relay`.
    pub fn spawn_subscriber(self: Arc<Self>, relay: Arc<Relay>) -> JoinHandle<()> {
        tokio::spawn(async move { self.run(relay).await })
    }
}

impl<T: AccessTokenSource + 'static> FrameBus for PubSubFrameBus<T> {
    #[cfg(not(tarpaulin_include))] // network I/O — request shapes are unit-tested
    fn publish<'a>(
        &'a self,
        to_instance: &'a str,
        session: &'a str,
        frame: BusFrame,
    ) -> FrameBusFuture<'a, ()> {
        Box::pin(async move { self.publish_frame(to_instance, session, &frame).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_client_frame() -> BusFrame {
        BusFrame::to_client("owner-x", 7, "frame")
    }

    #[test]
    fn publish_body_base64s_the_frame_with_attributes_and_composite_ordering_key() {
        let frame = to_client_frame();
        let body = publish_body("inst-b", "sess-1", &frame);
        let msg = &body["messages"][0];
        // Attributes carry the target instance + the RAW session slug (delivery
        // re-keys the local maps by it). The ordering key is the (owner, session)
        // COMPOSITE, not the bare slug, so two owners sharing a slug don't couple
        // their head-of-line ordering.
        assert_eq!(msg["attributes"]["to_instance"], "inst-b");
        assert_eq!(msg["attributes"]["session"], "sess-1");
        assert_eq!(
            msg["orderingKey"],
            crate::relay_registry::doc_id("owner-x", "sess-1")
        );
        // A different owner on the SAME slug yields a DISTINCT ordering key.
        let other = BusFrame::to_client("owner-y", 7, "frame");
        assert_ne!(
            publish_body("inst-b", "sess-1", &other)["messages"][0]["orderingKey"],
            msg["orderingKey"]
        );
        // The data is base64 of the frame JSON and round-trips back to the frame.
        let data = msg["data"].as_str().unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).unwrap();
        let round: BusFrame = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(round, frame);
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn publish_size_guard_admits_normal_frames_and_rejects_oversized_ones() {
        // A KB-scale frame is well within the bound.
        assert!(within_publish_limit(&to_client_frame()));
        // A frame whose body exceeds the guard is rejected (would 400 on Pub/Sub).
        let huge = BusFrame::to_client("owner-x", 1, "x".repeat(MAX_PUBLISH_BYTES));
        assert!(!within_publish_limit(&huge));
        // The encoded length accounts for base64's 4/3 expansion.
        assert!(encoded_body_len(&huge) >= MAX_PUBLISH_BYTES);
    }

    #[test]
    fn subscription_body_sets_ttl_and_message_ordering() {
        let topic = topic_resource("darkrun-app", "relay_frames");
        let body = subscription_body(&topic);
        assert_eq!(body["topic"], "projects/darkrun-app/topics/relay_frames");
        // Self-delete if the instance dies; ordered so a snapshot precedes updates.
        assert_eq!(body["expirationPolicy"]["ttl"], "86400s");
        assert_eq!(body["enableMessageOrdering"], true);
        assert_eq!(body["ackDeadlineSeconds"], 10);
    }

    #[test]
    fn subscription_id_and_resources_derive_from_the_instance() {
        assert_eq!(subscription_id("abc123"), "relay-frames-abc123");
        assert_eq!(
            subscription_resource("darkrun-app", "abc123"),
            "projects/darkrun-app/subscriptions/relay-frames-abc123"
        );
    }

    #[test]
    fn plan_delivery_drops_frames_for_other_instances_and_dispatches_ours() {
        let frame = to_client_frame();
        let data = base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&frame).unwrap());
        let message = serde_json::json!({
            "data": data,
            "attributes": { "to_instance": "inst-b", "session": "sess-1" },
        });

        // Addressed to us → dispatch with the decoded frame + session.
        assert_eq!(plan_delivery(&message, "inst-b"), Some(("sess-1".to_string(), frame)));
        // Addressed elsewhere → dropped (every subscription sees every frame).
        assert_eq!(plan_delivery(&message, "inst-a"), None);
        // Undecodable data addressed to us → dropped, not a panic.
        let bad = serde_json::json!({
            "data": "!!!not-base64!!!",
            "attributes": { "to_instance": "inst-b", "session": "sess-1" },
        });
        assert_eq!(plan_delivery(&bad, "inst-b"), None);
        // Missing attributes → dropped.
        let bare = serde_json::json!({ "data": "" });
        assert_eq!(plan_delivery(&bare, "inst-b"), None);
    }

    #[test]
    fn bus_frame_round_trips_both_directions() {
        let to_host = BusFrame::to_host("owner-x", "inst-b", HostEvent::Join { client: 9 });
        let j = serde_json::to_string(&to_host).unwrap();
        assert_eq!(serde_json::from_str::<BusFrame>(&j).unwrap(), to_host);

        let to_client = to_client_frame();
        let c = serde_json::to_string(&to_client).unwrap();
        assert_eq!(serde_json::from_str::<BusFrame>(&c).unwrap(), to_client);
    }

    #[tokio::test]
    async fn noop_bus_publishes_nothing() {
        // The no-op bus is infallible and carries nothing (single-instance).
        NoopFrameBus.publish("inst", "sess", to_client_frame()).await;
    }
}
