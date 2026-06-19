//! Push-acknowledgement payloads — a device confirming it received a gate push.
//!
//! The notify-and-await gate model surfaces a waiting review three ways
//! (presence, push, fall-back URL). Presence is the strongest signal; a push is
//! only known to have *landed* once the woken app posts back here. The engine's
//! `darkrun_await` then blocks with confidence that a human surface exists,
//! rather than guessing from a fire-and-forget push that may have been dropped.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Request body for `POST /api/push/ack` — a device, woken by a gate push,
/// confirming receipt for a session.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PushAckRequest {
    /// The session the push announced (a run slug or interactive session id).
    pub session_id: String,
    /// The FCM device token that received the push — the same token the device
    /// registered, so a later ack from the same device is idempotent.
    pub token: String,
}

/// Response body for `POST /api/push/ack` (200 on success).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PushAckResponse {
    /// Always `true` on success.
    pub ok: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_ack_request_roundtrips() {
        let req = PushAckRequest {
            session_id: "quiet-canyon".into(),
            token: "fcm-abc123".into(),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["session_id"], "quiet-canyon");
        assert_eq!(json["token"], "fcm-abc123");
        let back: PushAckRequest = serde_json::from_value(json).unwrap();
        assert_eq!(back.session_id, "quiet-canyon");
        assert_eq!(back.token, "fcm-abc123");
    }

    #[test]
    fn push_ack_request_requires_both_fields() {
        let bad = serde_json::json!({ "session_id": "r" });
        let parsed: Result<PushAckRequest, _> = serde_json::from_value(bad);
        assert!(parsed.is_err(), "token is required");
    }

    #[test]
    fn push_ack_response_serializes_ok() {
        let json = serde_json::to_value(PushAckResponse { ok: true }).unwrap();
        assert_eq!(json["ok"], true);
    }
}
