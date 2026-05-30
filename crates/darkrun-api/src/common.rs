//! Shared wire primitives used across session and feedback payloads.
//!
//! These are the small enums and structs referenced by more than one route
//! group: session status, the Checkpoint gate kind, the feedback taxonomy
//! (origin / severity / status / resolution / author), annotation bundles, and
//! the uniform validation-error envelope.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Runtime status spanning every interactive session type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Open, awaiting a decision/answer.
    #[default]
    Pending,
    /// A review decision was recorded.
    Decided,
    /// A question was answered.
    Answered,
    /// Approved.
    Approved,
    /// Changes were requested.
    ChangesRequested,
}

/// The kind of Checkpoint gate that opened a review session.
///
/// Mirrors [`darkrun_core::domain::CheckpointKind`] on the wire — kept as a
/// separate type so `darkrun-api` stays dependency-light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GateType {
    /// Advance automatically.
    Auto,
    /// Ask the local operator.
    Ask,
    /// Hand off to an external review surface.
    External,
    /// Block on a `darkrun_await_gate` call.
    Await,
}

/// The session-type discriminator — which kind of interactive session this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    /// A checkpoint review.
    Review,
    /// A multi-question prompt.
    Question,
    /// A design-direction selection.
    Direction,
    /// A blocking picker selection.
    Picker,
    /// A non-blocking artifact browser.
    View,
}

/// Authorship type derived from a feedback item's origin.
///
/// Human-authored feedback cannot be closed or deleted by workers; `system`
/// is reserved for manager-authored items (e.g. reject-loop escalation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthorType {
    /// Authored by a human reviewer.
    Human,
    /// Authored by a worker.
    Agent,
    /// Authored by the manager itself.
    System,
}

/// Where a feedback item originated.
///
/// Kept in lockstep with the on-disk feedback-origin enum so a projected item
/// never fails the wire parse. Drives [`AuthorType`] derivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackOrigin {
    /// A Reviewer adversarial pass.
    AdversarialReview,
    /// A factory-level review pass.
    StudioReview,
    /// A manager-built review role.
    EngineReview,
    /// A drift-sweep finding.
    Drift,
    /// An Explorer finding.
    Discovery,
    /// An external pull request.
    ExternalPr,
    /// An external merge request.
    ExternalMr,
    /// A human visual-pin annotation.
    UserVisual,
    /// A human chat message.
    UserChat,
    /// A human reply-seeking question.
    UserQuestion,
    /// A human revisit request.
    UserRevisit,
    /// A worker-authored item.
    Agent,
}

impl FeedbackOrigin {
    /// Derive the [`AuthorType`] this origin implies, mirroring the engine's
    /// `deriveAuthorType`. `user-*` origins are human; everything an agent or
    /// the manager produces is `agent`.
    pub fn author_type(self) -> AuthorType {
        match self {
            FeedbackOrigin::UserVisual
            | FeedbackOrigin::UserChat
            | FeedbackOrigin::UserQuestion
            | FeedbackOrigin::UserRevisit
            | FeedbackOrigin::ExternalPr
            | FeedbackOrigin::ExternalMr => AuthorType::Human,
            _ => AuthorType::Agent,
        }
    }
}

/// Finding urgency — drives fix-worker dispatch order.
///
/// Nullable on the wire: a human-authored item has no severity until the
/// classifier fix-worker backfills it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackSeverity {
    /// Stops the checkpoint.
    Blocker,
    /// Fix before delivery.
    High,
    /// Should fix.
    Medium,
    /// A nit.
    Low,
}

/// Lifecycle status of a feedback item.
///
/// `escalated` is reached when a worker-authored item's fix loop exceeds the
/// bolt cap without closure — a human-intervention waypoint, not a blocker.
/// Only `pending` and `fixing` block the checkpoint gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackStatus {
    /// Open, not yet picked up.
    Pending,
    /// A fix-worker is actively addressing it.
    Fixing,
    /// A fix has landed, awaiting verification.
    Addressed,
    /// Resolved by a reply, no code delta.
    Answered,
    /// Valid but needs no code fix.
    NonActionable,
    /// Bolt cap exceeded; awaiting a human.
    Escalated,
    /// Closed.
    Closed,
    /// Rejected.
    Rejected,
}

/// Routing hint for the manager's feedback resolver.
///
/// `None` on the wire means the caller has no preference and the router
/// defaults to `stage_revisit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackResolution {
    /// Skip the fix loop; answer the question.
    Question,
    /// Run a single fix-worker pass against the one finding.
    InlineFix,
    /// Re-loop the whole station.
    StageRevisit,
}

/// A reply on a feedback thread — answers a question, records a worker's
/// closure justification, or threads a short discussion.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackReply {
    /// Free-form author handle (`user`, a worker name).
    pub author: String,
    /// Derived authorship type.
    pub author_type: AuthorType,
    /// Reply body.
    pub body: String,
    /// ISO-8601 timestamp the reply was written.
    pub created_at: String,
}

/// A pin annotation dropped on a reviewed surface.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Pin {
    /// X coordinate (0..1 relative to surface width).
    pub x: f64,
    /// Y coordinate (0..1 relative to surface height).
    pub y: f64,
    /// Pin comment body.
    pub text: String,
}

/// An inline comment anchored to a span of text in a reviewed artifact.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InlineComment {
    /// The exact highlighted text the comment anchors to.
    pub selected_text: String,
    /// Comment body.
    pub comment: String,
    /// Zero-based paragraph index inside the reviewed artifact.
    pub paragraph: u32,
    /// Artifact path the comment was made on, relative to the run root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

/// Annotations attached to a review decision.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ReviewAnnotations {
    /// Base64-encoded PNG of the annotated canvas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
    /// Pin annotations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<Pin>,
    /// Inline comments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<InlineComment>,
}

/// A per-image pin annotation on a question session — like [`Pin`] but tagged
/// with which reference image it sits on.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QuestionPin {
    /// X coordinate (0..1 relative to image width).
    pub x: f64,
    /// Y coordinate (0..1 relative to image height).
    pub y: f64,
    /// Pin comment body.
    pub text: String,
    /// Index into the question's `image_urls[]` this pin sits on.
    pub image_index: u32,
}

/// One reviewer annotation pass over a question reference image — a comment
/// plus a screenshot of the captured surface.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QuestionScreenshotAnnotation {
    /// Reviewer's note on this annotation pass.
    pub comment: String,
    /// `data:image/png;base64,...` URL of the captured surface + strokes.
    pub screenshot_data_url: String,
    /// Index into the question's `image_urls[]` this was captured on.
    pub image_index: u32,
}

/// Annotation bundle attached to a question answer.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct QuestionAnnotations {
    /// Inline comments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<InlineComment>,
    /// Per-image pin annotations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<QuestionPin>,
    /// Per-pass screenshot annotations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screenshots: Vec<QuestionScreenshotAnnotation>,
}

/// A structural validation issue surfaced on the wire.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ValidationIssue {
    /// Machine-readable issue code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// JSON pointer-ish path to the offending field.
    pub path: Vec<String>,
}

/// The uniform `400` envelope returned whenever a request body fails schema
/// validation (including malformed JSON, surfaced as a synthetic issue).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ValidationError {
    /// Always `validation_failed`.
    pub error: String,
    /// The issues that caused the rejection.
    pub issues: Vec<ValidationIssue>,
}

impl ValidationError {
    /// Build a validation error from a list of issues, stamping the canonical
    /// `validation_failed` discriminator.
    pub fn new(issues: Vec<ValidationIssue>) -> Self {
        ValidationError {
            error: "validation_failed".to_string(),
            issues,
        }
    }
}

/// Default body-size cap for JSON request bodies (1 MiB).
pub const DEFAULT_BODY_MAX_BYTES: usize = 1_048_576;

/// Tighter cap for feedback update/delete endpoints (128 KiB).
pub const FEEDBACK_BODY_MAX_BYTES: usize = 131_072;

/// Larger cap for feedback create, which may carry an annotated screenshot
/// data URL (8 MiB).
pub const FEEDBACK_CREATE_MAX_BYTES: usize = 8_388_608;

/// Cap for question/direction submit routes carrying screenshot bundles
/// (32 MiB).
pub const SESSION_ANSWER_MAX_BYTES: usize = 33_554_432;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_author_type_derivation() {
        assert_eq!(FeedbackOrigin::UserChat.author_type(), AuthorType::Human);
        assert_eq!(FeedbackOrigin::ExternalPr.author_type(), AuthorType::Human);
        assert_eq!(FeedbackOrigin::Agent.author_type(), AuthorType::Agent);
        assert_eq!(
            FeedbackOrigin::AdversarialReview.author_type(),
            AuthorType::Agent
        );
        assert_eq!(FeedbackOrigin::Drift.author_type(), AuthorType::Agent);
    }

    #[test]
    fn feedback_origin_uses_kebab_case() {
        let json = serde_json::to_value(FeedbackOrigin::AdversarialReview).unwrap();
        assert_eq!(json, serde_json::json!("adversarial-review"));
        let json = serde_json::to_value(FeedbackOrigin::UserVisual).unwrap();
        assert_eq!(json, serde_json::json!("user-visual"));
    }

    #[test]
    fn validation_error_stamps_discriminator() {
        let err = ValidationError::new(vec![ValidationIssue {
            code: "invalid_json".into(),
            message: "bad".into(),
            path: vec![],
        }]);
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["error"], "validation_failed");
    }
}
