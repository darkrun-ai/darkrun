//! Design-direction select endpoint — `POST /direction/:id/select`.
//!
//! The submission is a discriminated union on `mode` with four arms:
//!
//! - `select`     — the user picked one archetype as the final direction.
//! - `regenerate` — the user wants fresh variants; `keep[]` names the
//!   archetypes to preserve.
//! - `upload`     — the designer supplied finished designs directly,
//!   skipping archetype generation.
//! - `generate`   — an intake signal: nothing to upload, generate variants.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::session::{DirectionPin, DirectionScreenshotAnnotation};

/// Annotation bundle attached to a direction selection.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DirectionAnnotations {
    /// Pin annotations on the rendered preview.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<DirectionPin>,
    /// Per-pass screenshot annotations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screenshots: Vec<DirectionScreenshotAnnotation>,
}

/// A single uploaded design file in an upload-mode submission.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DirectionUploadFile {
    /// Original filename (sanitised server-side).
    pub filename: String,
    /// `data:image/...;base64,...` URL of the uploaded file.
    pub data_url: String,
    /// Optional caption describing the artefact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

/// Request body for `POST /direction/:id/select` — discriminated on `mode`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum DirectionSelectRequest {
    /// Final selection — the user picked one archetype.
    Select {
        /// The chosen archetype name.
        archetype: String,
        /// Optional free-text comments.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comments: Option<String>,
        /// Optional annotations.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        annotations: Option<DirectionAnnotations>,
    },
    /// Regenerate request — the user wants more / different variants.
    Regenerate {
        /// Archetype names to preserve.
        keep: Vec<String>,
        /// Optional steering notes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comments: Option<String>,
    },
    /// Upload submission — finished designs supplied directly.
    Upload {
        /// The uploaded files.
        files: Vec<DirectionUploadFile>,
        /// Optional overall notes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comments: Option<String>,
    },
    /// Intake signal — nothing to upload; generate variants.
    Generate {
        /// Optional steering notes for the first generation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comments: Option<String>,
    },
}

/// Response body for `POST /direction/:id/select`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DirectionSelectResponse {
    /// Always `true` on success.
    pub ok: bool,
}

/// Request body for `POST /picker/:id/select`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PickerSelectRequest {
    /// The id of the option to select — must match one of the session's
    /// options.
    pub id: String,
}

/// Response body for `POST /picker/:id/select`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PickerSelectResponse {
    /// Always `true` on success.
    pub ok: bool,
    /// The selected option id, echoed back.
    pub id: String,
}
