//! Output-annotation review endpoint — `POST /visual-review/:id/annotate`.
//!
//! The operator gives a VISUAL REVIEW of a produced OUTPUT by annotating its
//! rendered screenshot — dropping pins (relative coordinate + note) and leaving
//! comments. Submitting the annotations records them onto the session payload
//! and produces a piece of `user-visual` FEEDBACK against the run/station, so
//! the fix-worker loop can act on it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::session::VisualReviewAnnotations;

/// Request body for `POST /visual-review/:id/annotate` — the operator's
/// annotations over the output screenshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct OutputReviewRequest {
    /// The pin + comment annotations on the output screenshot.
    pub annotations: VisualReviewAnnotations,
    /// Optional title for the produced feedback item (defaults to a
    /// surface-derived label server-side).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Response body for `POST /visual-review/:id/annotate` (201 on success).
///
/// Echoes the recorded annotation counts and the id of the FEEDBACK item the
/// annotation produced.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutputReviewResponse {
    /// Always `true` on success.
    pub ok: bool,
    /// The minted `FB-NN` feedback id the annotation produced.
    pub feedback_id: String,
    /// Number of pins recorded.
    pub pins: usize,
    /// Number of comments recorded.
    pub comments: usize,
}

impl OutputReviewRequest {
    /// Render the annotation bundle into a markdown feedback body — one pin per
    /// bullet (with its relative coordinate), then the free-text comments.
    pub fn to_feedback_body(&self) -> String {
        let mut out = String::new();
        for (i, pin) in self.annotations.pins.iter().enumerate() {
            out.push_str(&format!(
                "- Pin {} @ ({:.3}, {:.3}): {}\n",
                i + 1,
                pin.x,
                pin.y,
                pin.note
            ));
        }
        for comment in &self.annotations.comments {
            out.push_str(&format!("- {comment}\n"));
        }
        if out.is_empty() {
            out.push_str("(no annotations)\n");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{VisualReviewAnnotations, VisualReviewPin};

    fn sample() -> OutputReviewRequest {
        OutputReviewRequest {
            annotations: VisualReviewAnnotations {
                pins: vec![VisualReviewPin {
                    x: 0.5,
                    y: 0.25,
                    note: "button too small".into(),
                }],
                comments: vec!["overall good".into(), "fix the header".into()],
            },
            title: Some("home page review".into()),
        }
    }

    #[test]
    fn request_roundtrips() {
        let req = sample();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["annotations"]["pins"][0]["note"], "button too small");
        assert_eq!(json["annotations"]["comments"][1], "fix the header");
        assert_eq!(json["title"], "home page review");
        let back: OutputReviewRequest = serde_json::from_value(json).unwrap();
        assert_eq!(back.annotations.pins.len(), 1);
        assert_eq!(back.annotations.comments.len(), 2);
    }

    #[test]
    fn feedback_body_renders_pins_and_comments() {
        let body = sample().to_feedback_body();
        assert!(body.contains("Pin 1 @ (0.500, 0.250): button too small"));
        assert!(body.contains("- overall good"));
        assert!(body.contains("- fix the header"));
    }

    #[test]
    fn empty_annotations_render_placeholder() {
        let req = OutputReviewRequest::default();
        assert!(req.annotations.is_empty());
        assert_eq!(req.to_feedback_body(), "(no annotations)\n");
    }

    #[test]
    fn response_roundtrips() {
        let resp = OutputReviewResponse {
            ok: true,
            feedback_id: "FB-03".into(),
            pins: 1,
            comments: 2,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["feedback_id"], "FB-03");
        assert_eq!(json["pins"], 1);
        assert_eq!(json["comments"], 2);
    }
}
