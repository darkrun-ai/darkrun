//! Question-answer endpoint — `POST /question/:id/answer`.
//!
//! The wire schema for submitting answers to a multi-question session, plus the
//! success envelope.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::common::QuestionAnnotations;

/// A single question's answer in a multi-question submission.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QuestionAnswerItem {
    /// The question prompt (echoed back).
    pub question: String,
    /// The options the user selected.
    pub selected_options: Vec<String>,
    /// Free-text "other" input, when the question allows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_text: Option<String>,
}

/// Request body for `POST /question/:id/answer`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QuestionAnswerRequest {
    /// The answers, one per question.
    pub answers: Vec<QuestionAnswerItem>,
    /// Optional overall free-text feedback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    /// Optional annotation bundle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<QuestionAnnotations>,
}

/// Response body for `POST /question/:id/answer`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QuestionAnswerResponse {
    /// Always `true` on success.
    pub ok: bool,
}
