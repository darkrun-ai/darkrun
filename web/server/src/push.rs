//! Remote push — the REMOTE half of "notify as the engine ticks".
//!
//! Two complementary halves raise the same moment: the host fires a LOCAL OS
//! notification (darkrun-mcp's `notify`), and — when the operator is away from
//! that machine — the account's other devices get a REMOTE push. This module is
//! that remote half, living on the relay (the one component that already knows
//! every session's owner account).
//!
//! The shape:
//! - a device registers its FCM token with the web server on login/launch
//!   (`POST /devices`), keyed by the account the token resolves to;
//! - when the host signals a notify-worthy moment over the relay
//!   ([`HostCmd::Notify`](darkrun_api::tunnel::HostCmd::Notify)), the relay looks
//!   up the owner's registered devices and pushes to each over FCM.
//!
//! Both seams are traits so the relay fan-out is exercised fully offline (an
//! in-memory registry + a recording sender) and the production impls — a
//! persistent registry and the FCM HTTP v1 sender — wire in behind them.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// A registered device's push token plus the platform it runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceToken {
    /// The FCM registration token the push is addressed to.
    pub token: String,
    /// The platform (`ios`/`android`/`web`/`macos`), for diagnostics and (later)
    /// per-platform message shaping.
    pub platform: String,
}

/// The future a [`DeviceRegistry`] method returns — boxed so the trait stays
/// object-safe (`dyn DeviceRegistry`) while a network-backed impl (Firestore)
/// does async I/O, without pulling in an async-trait dependency.
pub type RegistryFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Maps each account to its registered devices, so a push fans out to all of an
/// owner's devices at once. Behind a trait so the in-memory impl tests offline
/// and a persistent (Firestore) impl can wire in for production unchanged. The
/// methods are async (boxed futures) because the production impl talks to
/// Firestore over HTTP.
pub trait DeviceRegistry: Send + Sync {
    /// Register (or refresh) `device` for `account`. Idempotent on the token:
    /// re-registering an existing token updates its platform, never duplicates.
    fn register<'a>(&'a self, account: &'a str, device: DeviceToken) -> RegistryFuture<'a, ()>;
    /// Drop a device token (logout / token rotation / uninstall). Idempotent —
    /// an unknown token is a silent no-op. This is the UNSCOPED drop; the public
    /// HTTP path must use [`unregister_for`](Self::unregister_for) so one account
    /// can't delete another's device.
    fn unregister<'a>(&'a self, token: &'a str) -> RegistryFuture<'a, ()>;
    /// Every device currently registered to `account` (empty if none).
    fn devices_for<'a>(&'a self, account: &'a str) -> RegistryFuture<'a, Vec<DeviceToken>>;

    /// Drop `token` **only if** it is registered to `account` — the
    /// ownership-scoped unregister the `DELETE /devices/{token}` handler uses.
    /// Without this an authenticated caller could pass any token and unregister a
    /// stranger's device, silently disabling their gate push. Idempotent: a token
    /// the account doesn't own (or that doesn't exist) is a no-op. The default
    /// verifies ownership via [`devices_for`](Self::devices_for) then drops;
    /// impls may override with a narrower query.
    fn unregister_for<'a>(
        &'a self,
        account: &'a str,
        token: &'a str,
    ) -> RegistryFuture<'a, ()> {
        Box::pin(async move {
            if self.devices_for(account).await.iter().any(|d| d.token == token) {
                self.unregister(token).await;
            }
        })
    }
}

/// An in-memory registry: `account -> {token -> platform}`. Stateless across
/// restarts — devices re-register on the next login/launch, so a relay restart
/// just means a brief window where a push finds no devices.
#[derive(Default)]
pub struct InMemoryDeviceRegistry {
    by_account: Mutex<HashMap<String, HashMap<String, String>>>,
}

impl InMemoryDeviceRegistry {
    /// A fresh, empty registry.
    pub fn new() -> Self {
        Self::default()
    }
}

impl DeviceRegistry for InMemoryDeviceRegistry {
    fn register<'a>(&'a self, account: &'a str, device: DeviceToken) -> RegistryFuture<'a, ()> {
        self.by_account
            .lock()
            .unwrap()
            .entry(account.to_string())
            .or_default()
            .insert(device.token, device.platform);
        Box::pin(async {})
    }

    fn unregister<'a>(&'a self, token: &'a str) -> RegistryFuture<'a, ()> {
        // A token belongs to exactly one account, but we don't index by token,
        // so sweep — registries are small (a person's handful of devices).
        self.by_account.lock().unwrap().retain(|_, devices| {
            devices.remove(token);
            !devices.is_empty()
        });
        Box::pin(async {})
    }

    fn devices_for<'a>(&'a self, account: &'a str) -> RegistryFuture<'a, Vec<DeviceToken>> {
        let out = self
            .by_account
            .lock()
            .unwrap()
            .get(account)
            .map(|devices| {
                devices
                    .iter()
                    .map(|(token, platform)| DeviceToken {
                        token: token.clone(),
                        platform: platform.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Box::pin(async move { out })
    }
}

/// The future a [`PushSender`] returns — boxed so the trait stays object-safe
/// (`dyn PushSender`) without pulling in an async-trait dependency.
pub type PushFuture<'a> = Pin<Box<dyn Future<Output = usize> + Send + 'a>>;

/// Delivers a notification to a set of devices, returning how many were sent
/// successfully. Behind a trait so the relay fan-out is tested with a recording
/// sender and the FCM HTTP v1 sender wires in for production.
pub trait PushSender: Send + Sync {
    /// Push `title`/`body` to every device in `devices`. Best-effort: a failed
    /// device send is skipped, not fatal — the count returned is the successes.
    fn push<'a>(&'a self, devices: &'a [DeviceToken], title: &'a str, body: &'a str)
        -> PushFuture<'a>;
}

/// A sender that drops every push — the safe default when no FCM credentials are
/// configured (the local OS notification still fires; only the remote half is a
/// no-op). Keeps the relay wired the same way in every environment.
pub struct NoopPushSender;

impl PushSender for NoopPushSender {
    fn push<'a>(
        &'a self,
        _devices: &'a [DeviceToken],
        _title: &'a str,
        _body: &'a str,
    ) -> PushFuture<'a> {
        Box::pin(async { 0 })
    }
}

/// Build the FCM HTTP v1 message body for one device. Pure, so the wire shape is
/// unit-tested without sending anything. Shape per the FCM v1 `Message`:
/// `{ "message": { "token": ..., "notification": { "title", "body" } } }`.
pub fn fcm_message(token: &str, title: &str, body: &str) -> serde_json::Value {
    serde_json::json!({
        "message": {
            "token": token,
            "notification": { "title": title, "body": body },
        }
    })
}

/// The FCM HTTP v1 send endpoint for a project.
pub fn fcm_endpoint(project_id: &str) -> String {
    format!("https://fcm.googleapis.com/v1/projects/{project_id}/messages:send")
}

/// Supplies a Google OAuth2 access token for the FCM `messages:send` scope.
/// Behind a trait so [`FcmPushSender`] is tested with a static token and the
/// production source (a service-account JWT exchanged for a short-lived,
/// cached access token) wires in unchanged.
pub trait AccessTokenSource: Send + Sync {
    /// A bearer access token, or an error string when one can't be obtained.
    fn access_token(&self) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;
}

/// An [`AccessTokenSource`] that returns a fixed token — for tests and local
/// runs where the token is provided out of band.
pub struct StaticTokenSource(pub String);

impl AccessTokenSource for StaticTokenSource {
    fn access_token(&self) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
        let token = self.0.clone();
        Box::pin(async move { Ok(token) })
    }
}

/// The production [`PushSender`]: POSTs an FCM HTTP v1 message per device,
/// authorized by an access token from the injected [`AccessTokenSource`].
pub struct FcmPushSender<T: AccessTokenSource> {
    project_id: String,
    tokens: T,
    http: reqwest::Client,
}

impl<T: AccessTokenSource> FcmPushSender<T> {
    /// An FCM sender for `project_id`, minting access tokens from `tokens`.
    pub fn new(project_id: impl Into<String>, tokens: T) -> Self {
        Self {
            project_id: project_id.into(),
            tokens,
            http: reqwest::Client::new(),
        }
    }
}

impl<T: AccessTokenSource> PushSender for FcmPushSender<T> {
    fn push<'a>(
        &'a self,
        devices: &'a [DeviceToken],
        title: &'a str,
        body: &'a str,
    ) -> PushFuture<'a> {
        Box::pin(async move {
            if devices.is_empty() {
                return 0;
            }
            let access = match self.tokens.access_token().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(error = %e, "FCM access token unavailable; skipping push");
                    return 0;
                }
            };
            let endpoint = fcm_endpoint(&self.project_id);
            let mut sent = 0;
            for device in devices {
                let body_json = fcm_message(&device.token, title, body);
                match self
                    .http
                    .post(&endpoint)
                    .bearer_auth(&access)
                    .json(&body_json)
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => sent += 1,
                    Ok(resp) => {
                        let status = resp.status();
                        tracing::warn!(%status, platform = %device.platform, "FCM push rejected");
                    }
                    Err(e) => tracing::warn!(error = %e, "FCM push request failed"),
                }
            }
            sent
        })
    }
}

/// Fan a notification out to every device registered to `account`: look the
/// devices up in `registry` and hand them to `sender`. Returns the number of
/// devices the sender reports delivered. The shared helper behind the relay's
/// [`HostCmd::Notify`](darkrun_api::tunnel::HostCmd::Notify) handling.
pub async fn fan_out(
    registry: &Arc<dyn DeviceRegistry>,
    sender: &Arc<dyn PushSender>,
    account: &str,
    title: &str,
    body: &str,
) -> usize {
    let devices = registry.devices_for(account).await;
    if devices.is_empty() {
        return 0;
    }
    sender.push(&devices, title, body).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(token: &str, platform: &str) -> DeviceToken {
        DeviceToken { token: token.into(), platform: platform.into() }
    }

    /// A sender that records every fan-out it's asked to perform.
    #[derive(Default)]
    struct RecordingPushSender {
        calls: Mutex<Vec<(Vec<String>, String, String)>>,
    }

    impl PushSender for RecordingPushSender {
        fn push<'a>(
            &'a self,
            devices: &'a [DeviceToken],
            title: &'a str,
            body: &'a str,
        ) -> PushFuture<'a> {
            let tokens: Vec<String> = devices.iter().map(|d| d.token.clone()).collect();
            self.calls
                .lock()
                .unwrap()
                .push((tokens.clone(), title.to_string(), body.to_string()));
            Box::pin(async move { tokens.len() })
        }
    }

    #[tokio::test]
    async fn registry_registers_refreshes_and_unregisters() {
        let reg = InMemoryDeviceRegistry::new();
        reg.register("acct-a", dev("t1", "ios")).await;
        reg.register("acct-a", dev("t2", "web")).await;
        reg.register("acct-b", dev("t3", "android")).await;

        let mut a = reg.devices_for("acct-a").await;
        a.sort_by(|x, y| x.token.cmp(&y.token));
        assert_eq!(a, vec![dev("t1", "ios"), dev("t2", "web")]);
        assert_eq!(reg.devices_for("acct-b").await, vec![dev("t3", "android")]);

        // Re-registering a token refreshes its platform, never duplicates.
        reg.register("acct-a", dev("t1", "macos")).await;
        assert_eq!(reg.devices_for("acct-a").await.len(), 2);
        assert!(reg.devices_for("acct-a").await.contains(&dev("t1", "macos")));

        // Unregister drops just that token; emptying an account removes it.
        reg.unregister("t3").await;
        assert!(reg.devices_for("acct-b").await.is_empty());
        reg.unregister("t1").await;
        assert_eq!(reg.devices_for("acct-a").await, vec![dev("t2", "web")]);

        // Unknown token is a no-op.
        reg.unregister("nope").await;
        assert_eq!(reg.devices_for("acct-a").await.len(), 1);
    }

    #[tokio::test]
    async fn unregister_for_only_drops_the_owning_accounts_token() {
        let reg = InMemoryDeviceRegistry::new();
        reg.register("owner", dev("t1", "ios")).await;
        reg.register("intruder", dev("t2", "web")).await;

        // An account that doesn't own the token can't drop it.
        reg.unregister_for("intruder", "t1").await;
        assert_eq!(reg.devices_for("owner").await, vec![dev("t1", "ios")]);

        // The owner can.
        reg.unregister_for("owner", "t1").await;
        assert!(reg.devices_for("owner").await.is_empty());
        // The intruder's own device is untouched throughout.
        assert_eq!(reg.devices_for("intruder").await, vec![dev("t2", "web")]);
    }

    #[test]
    fn fcm_message_has_the_v1_shape() {
        let m = fcm_message("tok-1", "darkrun · run", "Build needs you.");
        assert_eq!(m["message"]["token"], "tok-1");
        assert_eq!(m["message"]["notification"]["title"], "darkrun · run");
        assert_eq!(m["message"]["notification"]["body"], "Build needs you.");
    }

    #[test]
    fn fcm_endpoint_targets_the_project() {
        assert_eq!(
            fcm_endpoint("darkrun-app"),
            "https://fcm.googleapis.com/v1/projects/darkrun-app/messages:send"
        );
    }

    #[tokio::test]
    async fn fan_out_pushes_to_an_accounts_devices_only() {
        let registry: Arc<dyn DeviceRegistry> = Arc::new(InMemoryDeviceRegistry::new());
        registry.register("owner", dev("t1", "ios")).await;
        registry.register("owner", dev("t2", "web")).await;
        registry.register("other", dev("t3", "android")).await;
        let recorder = Arc::new(RecordingPushSender::default());
        let sender: Arc<dyn PushSender> = recorder.clone();

        let n = fan_out(&registry, &sender, "owner", "T", "B").await;
        assert_eq!(n, 2, "both of owner's devices, none of other's");

        let calls = recorder.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let mut tokens = calls[0].0.clone();
        tokens.sort();
        assert_eq!(tokens, vec!["t1".to_string(), "t2".to_string()]);
        assert_eq!((calls[0].1.as_str(), calls[0].2.as_str()), ("T", "B"));
    }

    #[tokio::test]
    async fn fan_out_to_an_account_with_no_devices_sends_nothing() {
        let registry: Arc<dyn DeviceRegistry> = Arc::new(InMemoryDeviceRegistry::new());
        let recorder = Arc::new(RecordingPushSender::default());
        let sender: Arc<dyn PushSender> = recorder.clone();

        assert_eq!(fan_out(&registry, &sender, "ghost", "T", "B").await, 0);
        assert!(recorder.calls.lock().unwrap().is_empty(), "no devices → no send");
    }

    #[tokio::test]
    async fn noop_sender_delivers_nothing() {
        let sender = NoopPushSender;
        assert_eq!(sender.push(&[dev("t", "ios")], "T", "B").await, 0);
    }

    #[tokio::test]
    async fn static_token_source_returns_its_token() {
        let src = StaticTokenSource("ya29.test".into());
        assert_eq!(src.access_token().await.unwrap(), "ya29.test");
    }
}
