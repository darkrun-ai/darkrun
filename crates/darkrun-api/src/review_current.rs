//! Run-state summary endpoint — `GET /api/review/current`.
//!
//! A compact projection of "where is the active run, and what's outstanding?"
//! the desktop app polls to render the run header and feedback counts without
//! pulling the full review payload.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Per-status counts of feedback items for the active station.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackSummary {
    /// Items still open.
    pub pending: u32,
    /// Items with a landed fix awaiting verification.
    pub addressed: u32,
    /// Items closed.
    pub closed: u32,
    /// Items rejected.
    pub rejected: u32,
}

/// A station entry in the run-state summary.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReviewCurrentStation {
    /// The station name.
    pub name: String,
    /// Lifecycle status (display).
    pub status: String,
    /// Current phase (display).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// The current Pass iteration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iteration: Option<u32>,
    /// The number of times the station has been visited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visits: Option<u32>,
}

/// A unit entry in the run-state summary.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReviewCurrentUnit {
    /// The unit slug.
    pub slug: String,
    /// The unit title.
    pub title: String,
    /// Lifecycle status (display).
    pub status: String,
}

/// `GET /api/review/current` response body.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReviewCurrentPayload {
    /// The active run slug.
    pub run: String,
    /// The active station, or null when between stations.
    pub station: Option<String>,
    /// The active phase (display).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// The units in the active station.
    pub units: Vec<ReviewCurrentUnit>,
    /// Per-status feedback counts.
    pub feedback_summary: FeedbackSummary,
    /// All stations on the run.
    pub stations: Vec<ReviewCurrentStation>,
}
