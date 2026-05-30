//! Advance endpoint — `POST /api/advance/:id`.
//!
//! The desktop app's wake signal to the manager. No request body. Two effects
//! per call: if no feedback is open on the resolved station, the user
//! review/approval slots are stamped (the act of advancing with nothing
//! pending IS the approval); and the gate session's pending decision is set so
//! the parked `darkrun_await_gate` waiter unblocks.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Response body for `POST /api/advance/:id`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AdvanceResponse {
    /// Always `true` on success.
    pub ok: bool,
    /// The station the advance signal resolved against.
    pub station: String,
    /// Number of pending / fixing / addressed feedback items on the station at
    /// the time of the call. Zero means the user slots were stamped.
    pub open_feedback_count: u32,
    /// True when this call stamped the user review/approval slots.
    pub stamped_user_slots: bool,
}
