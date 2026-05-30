//! Review-decision request/response — `POST /review/:id/decide`.
//!
//! The wire schema for recording a review decision.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::common::ReviewAnnotations;

/// The canonical review decision. Anything other than `approved` is coerced
/// to `changes_requested` server-side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    /// The reviewer approved the work.
    Approved,
    /// The reviewer requested changes.
    ChangesRequested,
}

/// Request body for `POST /review/:id/decide`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReviewDecisionRequest {
    /// Raw decision string — the server canonicalizes to one of the
    /// [`ReviewDecision`] variants.
    pub decision: String,
    /// Optional reviewer free-text feedback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    /// Optional annotations attached to the decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ReviewAnnotations>,
}

/// Response body for `POST /review/:id/decide`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReviewDecisionResponse {
    /// Always `true` on success.
    pub ok: bool,
    /// The canonicalized decision.
    pub decision: ReviewDecision,
    /// The feedback echoed back (empty when none was supplied).
    pub feedback: String,
}

impl ReviewDecision {
    /// Canonicalize a raw decision string: only an exact `approved`
    /// (case-insensitive) yields [`ReviewDecision::Approved`].
    pub fn canonicalize(raw: &str) -> ReviewDecision {
        if raw.trim().eq_ignore_ascii_case("approved") {
            ReviewDecision::Approved
        } else {
            ReviewDecision::ChangesRequested
        }
    }
}
