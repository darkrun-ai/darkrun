//! Feedback CRUD request/response wire types — `/api/feedback/...`.
//!
//! The desktop review app reads, creates, updates, and deletes Feedback items
//! routed back from a Checkpoint. Feedback lives on disk as a markdown file
//! with YAML frontmatter under a run's `feedback/` directory; these types are
//! the JSON envelopes the HTTP server exchanges with the app.
//!
//! The lifecycle [`FeedbackStatus`], [`FeedbackOrigin`], [`FeedbackSeverity`],
//! and [`FeedbackResolution`] taxonomies are shared with the session payloads
//! and live in [`crate::common`]; [`FeedbackStatus::canonicalize`] folds an
//! arbitrary on-disk token onto a known variant.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::common::{
    AuthorType, FeedbackOrigin, FeedbackReply, FeedbackResolution, FeedbackSeverity,
    FeedbackStatus,
};

impl FeedbackStatus {
    /// Canonicalize a raw status string. Unknown values fall back to `pending`.
    pub fn canonicalize(raw: &str) -> FeedbackStatus {
        match raw.trim().to_ascii_lowercase().as_str() {
            "fixing" => FeedbackStatus::Fixing,
            "addressed" => FeedbackStatus::Addressed,
            "answered" => FeedbackStatus::Answered,
            "non_actionable" => FeedbackStatus::NonActionable,
            "escalated" => FeedbackStatus::Escalated,
            "closed" => FeedbackStatus::Closed,
            "rejected" => FeedbackStatus::Rejected,
            _ => FeedbackStatus::Pending,
        }
    }

    /// The frontmatter token for this status.
    pub fn as_str(self) -> &'static str {
        match self {
            FeedbackStatus::Pending => "pending",
            FeedbackStatus::Fixing => "fixing",
            FeedbackStatus::Addressed => "addressed",
            FeedbackStatus::Answered => "answered",
            FeedbackStatus::NonActionable => "non_actionable",
            FeedbackStatus::Escalated => "escalated",
            FeedbackStatus::Closed => "closed",
            FeedbackStatus::Rejected => "rejected",
        }
    }

    /// Whether this status still blocks the Checkpoint gate. Only `pending` and
    /// `fixing` hold the gate; `escalated` is a human-intervention waypoint, not
    /// a blocker.
    pub fn blocks_gate(self) -> bool {
        matches!(self, FeedbackStatus::Pending | FeedbackStatus::Fixing)
    }

    /// Whether this item still needs the operator's attention (it is not
    /// terminally resolved). Broader than [`blocks_gate`]: it ALSO counts
    /// `escalated`, the explicit human-intervention state, so a surface like the
    /// drift chip shows an escalated item instead of dropping it. Terminal
    /// resolutions (`addressed` / `answered` / `non_actionable` / `closed` /
    /// `rejected`) are not open.
    pub fn is_open(self) -> bool {
        matches!(
            self,
            FeedbackStatus::Pending | FeedbackStatus::Fixing | FeedbackStatus::Escalated
        )
    }
}

/// Inline text-anchor metadata for inline-comment feedback.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackInlineAnchor {
    /// The exact highlighted text the comment anchors to.
    pub selected_text: String,
    /// Zero-based paragraph index inside the reviewed artifact.
    pub paragraph: u32,
    /// Human-readable label shown in the feedback card.
    pub location: String,
    /// DOM id of the saved highlight span.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_id: Option<String>,
    /// Full repo-root-relative path to the anchored artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Content hash at save time, for drift detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha: Option<String>,
}

/// The result of one fix-worker pass against a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IterationResult {
    /// The worker finished and handed off to the next.
    Advanced,
    /// The validator verified resolution and closed the finding.
    Closed,
    /// The validator rejected the fix; the bolt budget was spent.
    Reopened,
    /// A worker dismissed the finding.
    Rejected,
}

/// One bolt in a finding's fix-loop history.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackIteration {
    /// The bolt index.
    pub bolt: u32,
    /// The fix-worker role that fired.
    pub hat: String,
    /// When the pass started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// When the pass completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// The transition result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<IterationResult>,
    /// Git SHA of the commit the worker produced, when one was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The handoff baton recorded on this transition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Deprecated legacy reject reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A worker's plain-language reply at closure time.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClosureReply {
    /// The reply text.
    pub text: String,
    /// When it was written.
    pub at: String,
}

/// The scope a feedback item lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackScope {
    /// Run-scope (logged by the run-level completion review).
    Intent,
    /// Station-scope (the normal adversarial review output).
    Stage,
}

/// A single feedback item as projected onto the wire.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackItem {
    /// Stable `FB-NN` identifier (scoped per station).
    pub feedback_id: String,
    /// Short title.
    #[serde(default)]
    pub title: String,
    /// Markdown body.
    #[serde(default)]
    pub body: String,
    /// Lifecycle status.
    pub status: FeedbackStatus,
    /// Where the item originated.
    pub origin: FeedbackOrigin,
    /// Finding urgency (null until classified).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<FeedbackSeverity>,
    /// Free-form author handle (e.g. `user`, `agent`).
    pub author: String,
    /// Derived authorship type.
    pub author_type: AuthorType,
    /// ISO-8601 creation timestamp.
    pub created_at: String,
    /// Station-visit counter at creation time.
    pub visit: u32,
    /// Back-reference to the origin artifact, or null.
    pub source_ref: Option<String>,
    /// Unit slug that certified closure, or `None` while open.
    pub closed_by: Option<String>,
    /// Routing hint for the resolver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<FeedbackResolution>,
    /// Thread of replies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<FeedbackReply>,
    /// Inline-text anchor metadata, when text-anchored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_anchor: Option<FeedbackInlineAnchor>,
    /// Whether the item is run- or station-scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<FeedbackScope>,
    /// Per-bolt fix-loop history.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub iterations: Vec<FeedbackIteration>,
    /// The worker's plain-language closure reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closure_reply: Option<ClosureReply>,
    /// Whether the closure reply is unacknowledged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closure_reply_unread: Option<bool>,
}

/// `GET /api/feedback/:run/:station` response body.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackListResponse {
    /// The run (intent) slug.
    pub run: String,
    /// The station.
    pub station: String,
    /// Number of items returned.
    pub count: usize,
    /// The feedback items.
    pub items: Vec<FeedbackItem>,
}

/// Pin-anchor metadata for visual (pin-drop) annotations on a create request.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackAnchor {
    /// The page id the pin sits on.
    pub page_id: String,
    /// X coordinate (0..1 of viewport width).
    pub x: f64,
    /// Y coordinate (0..1 of viewport height).
    pub y: f64,
    /// Viewport width in pixels at drop time.
    pub viewport_width: u32,
    /// Viewport height in pixels at drop time.
    pub viewport_height: u32,
}

/// `POST /api/feedback/:run/:station` request body.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackCreateRequest {
    /// Title (required, non-empty).
    pub title: String,
    /// Body (required, non-empty).
    pub body: String,
    /// Origin (defaults to `user-visual` server-side when omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<FeedbackOrigin>,
    /// Optional author hint (the server always stamps `user` for HTTP
    /// submissions, so this is retained for compatibility only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Back-reference to an origin artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    /// Pin-anchor metadata for visual annotations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<FeedbackAnchor>,
    /// Inline text-anchor metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_anchor: Option<FeedbackInlineAnchor>,
    /// The author's preferred resolution path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<FeedbackResolution>,
    /// Optional PNG/JPEG/WebP attachment as a data URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_data_url: Option<String>,
}

/// `POST /api/feedback/:run/:station` response body (201 on success).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackCreateResponse {
    /// The minted `FB-NN` id.
    pub feedback_id: String,
    /// Path to the committed feedback file, relative to the run dir.
    pub file: String,
    /// Always `pending` on create.
    pub status: FeedbackStatus,
    /// Human-readable confirmation message.
    pub message: String,
}

/// `PUT /api/feedback/:run/:station/:id` request body.
///
/// At least one of `status` / `closed_by` / `resolution` must be supplied; an
/// empty body is a `400`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackUpdateRequest {
    /// New status, if changing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<FeedbackStatus>,
    /// Unit slug certifying closure, if changing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_by: Option<String>,
    /// New resolution hint, if changing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<FeedbackResolution>,
}

impl FeedbackUpdateRequest {
    /// Whether the request carries no mutating field — a `400` at the handler.
    pub fn is_empty(&self) -> bool {
        self.status.is_none() && self.closed_by.is_none() && self.resolution.is_none()
    }
}

/// `PUT /api/feedback/:run/:station/:id` response body.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackUpdateResponse {
    /// The updated id.
    pub feedback_id: String,
    /// Frontmatter fields that were actually changed.
    pub updated_fields: Vec<String>,
    /// Human-readable confirmation message.
    pub message: String,
}

/// `DELETE /api/feedback/:run/:station/:id` response body.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackDeleteResponse {
    /// The deleted id.
    pub feedback_id: String,
    /// Always `true` on success.
    pub deleted: bool,
    /// Human-readable confirmation message.
    pub message: String,
}

/// `POST /api/feedback/:run/:station/:id/replies` request body.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackReplyCreateRequest {
    /// Reply body (required, non-empty).
    pub body: String,
    /// Optional author hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// If `true`, transitions the parent to `answered` in the same write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_as_answered: Option<bool>,
}

/// `POST /api/feedback/:run/:station/:id/replies` response body.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackReplyCreateResponse {
    /// The parent id.
    pub feedback_id: String,
    /// Zero-based index of the appended reply.
    pub reply_index: usize,
    /// The parent status after the write.
    pub status: FeedbackStatus,
    /// Human-readable confirmation message.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_canonicalizes() {
        assert_eq!(
            FeedbackStatus::canonicalize("FIXING"),
            FeedbackStatus::Fixing
        );
        assert_eq!(
            FeedbackStatus::canonicalize("answered"),
            FeedbackStatus::Answered
        );
        assert_eq!(
            FeedbackStatus::canonicalize("Closed"),
            FeedbackStatus::Closed
        );
        assert_eq!(
            FeedbackStatus::canonicalize("escalated"),
            FeedbackStatus::Escalated
        );
        assert_eq!(
            FeedbackStatus::canonicalize("non_actionable"),
            FeedbackStatus::NonActionable
        );
        // Unknown falls back to pending.
        assert_eq!(
            FeedbackStatus::canonicalize("weird"),
            FeedbackStatus::Pending
        );
    }

    #[test]
    fn status_roundtrips_as_str() {
        for s in [
            FeedbackStatus::Pending,
            FeedbackStatus::Fixing,
            FeedbackStatus::Addressed,
            FeedbackStatus::Answered,
            FeedbackStatus::NonActionable,
            FeedbackStatus::Escalated,
            FeedbackStatus::Closed,
            FeedbackStatus::Rejected,
        ] {
            assert_eq!(FeedbackStatus::canonicalize(s.as_str()), s);
        }
    }

    #[test]
    fn gate_blocking() {
        assert!(FeedbackStatus::Pending.blocks_gate());
        assert!(FeedbackStatus::Fixing.blocks_gate());
        assert!(!FeedbackStatus::Escalated.blocks_gate());
        assert!(!FeedbackStatus::Closed.blocks_gate());
    }

    #[test]
    fn update_request_emptiness() {
        assert!(FeedbackUpdateRequest::default().is_empty());
        assert!(!FeedbackUpdateRequest {
            status: Some(FeedbackStatus::Closed),
            ..Default::default()
        }
        .is_empty());
        assert!(!FeedbackUpdateRequest {
            resolution: Some(FeedbackResolution::InlineFix),
            ..Default::default()
        }
        .is_empty());
    }

    #[test]
    fn create_response_roundtrips() {
        let resp = FeedbackCreateResponse {
            feedback_id: "FB-01".into(),
            file: "feedback/FB-01.md".into(),
            status: FeedbackStatus::Pending,
            message: "created".into(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "pending");
        assert_eq!(json["feedback_id"], "FB-01");
    }

    #[test]
    fn feedback_item_roundtrips() {
        let item = FeedbackItem {
            feedback_id: "FB-07".into(),
            title: "fix the thing".into(),
            body: "details".into(),
            status: FeedbackStatus::Pending,
            origin: FeedbackOrigin::UserVisual,
            severity: Some(FeedbackSeverity::High),
            author: "user".into(),
            author_type: AuthorType::Human,
            created_at: "2026-05-30T00:00:00Z".into(),
            visit: 1,
            source_ref: None,
            closed_by: None,
            resolution: Some(FeedbackResolution::StageRevisit),
            replies: vec![],
            inline_anchor: None,
            scope: Some(FeedbackScope::Stage),
            iterations: vec![],
            closure_reply: None,
            closure_reply_unread: None,
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["origin"], "user-visual");
        assert_eq!(json["author_type"], "human");
        assert_eq!(json["severity"], "high");
        let back: FeedbackItem = serde_json::from_value(json).unwrap();
        assert_eq!(back.feedback_id, "FB-07");
        assert_eq!(back.status, FeedbackStatus::Pending);
    }
}
