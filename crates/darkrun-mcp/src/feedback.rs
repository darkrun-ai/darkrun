//! Typed feedback layer over the core raw-feedback store.
//!
//! `darkrun-core` keeps feedback opaque (a `feedback/*.md` doc with a YAML
//! frontmatter fence over a markdown body). This module gives the MCP tools a
//! typed read/write surface for those documents — create, list, resolve,
//! move, and reject — built on
//! [`StateStore::read_feedback_raw`](darkrun_core::StateStore::read_feedback_raw)
//! and [`write_feedback_raw`](darkrun_core::StateStore::write_feedback_raw).
//!
//! The on-disk shape is intentionally simple YAML frontmatter so that the
//! manager's `feedback_open` walk (in [`crate::position`]) and these tools
//! agree on the same `status:` line.

use chrono::Utc;
use darkrun_core::domain::{Feedback, FeedbackSeverity, FeedbackStatus};
use darkrun_core::StateStore;

use crate::error::{McpError, Result};

/// The terminal statuses — a feedback item in one of these is "settled" and
/// the manager's feedback walk will stop re-dispatching it.
pub const TERMINAL: &[FeedbackStatus] = &[
    FeedbackStatus::Addressed,
    FeedbackStatus::Answered,
    FeedbackStatus::NonActionable,
    FeedbackStatus::Closed,
    FeedbackStatus::Rejected,
];

/// Whether a status is terminal (settled).
pub fn is_terminal(status: FeedbackStatus) -> bool {
    TERMINAL.contains(&status)
}

fn status_str(status: FeedbackStatus) -> &'static str {
    match status {
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

fn parse_status(raw: &str) -> Option<FeedbackStatus> {
    match raw.trim().trim_matches('"').to_ascii_lowercase().as_str() {
        "pending" => Some(FeedbackStatus::Pending),
        "fixing" => Some(FeedbackStatus::Fixing),
        "addressed" => Some(FeedbackStatus::Addressed),
        "answered" => Some(FeedbackStatus::Answered),
        "non_actionable" | "nonactionable" => Some(FeedbackStatus::NonActionable),
        "escalated" => Some(FeedbackStatus::Escalated),
        "closed" => Some(FeedbackStatus::Closed),
        "rejected" => Some(FeedbackStatus::Rejected),
        _ => None,
    }
}

fn severity_str(severity: FeedbackSeverity) -> &'static str {
    match severity {
        FeedbackSeverity::Blocker => "blocker",
        FeedbackSeverity::High => "high",
        FeedbackSeverity::Medium => "medium",
        FeedbackSeverity::Low => "low",
    }
}

/// Parse a severity name, accepting the canonical snake_case form.
pub fn parse_severity(raw: &str) -> Option<FeedbackSeverity> {
    match raw.trim().trim_matches('"').to_ascii_lowercase().as_str() {
        "blocker" => Some(FeedbackSeverity::Blocker),
        "high" => Some(FeedbackSeverity::High),
        "medium" => Some(FeedbackSeverity::Medium),
        "low" => Some(FeedbackSeverity::Low),
        _ => None,
    }
}

/// Serialize a feedback item to its on-disk `feedback/*.md` shape.
fn serialize(fb: &Feedback) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("status: {}\n", status_str(fb.status)));
    out.push_str(&format!("station: {}\n", fb.station));
    if let Some(sev) = fb.severity {
        out.push_str(&format!("severity: {}\n", severity_str(sev)));
    }
    if let Some(created) = &fb.created_at {
        out.push_str(&format!("created_at: {created}\n"));
    }
    out.push_str("---\n");
    out.push_str(&fb.body);
    if !fb.body.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Parse a raw `feedback/*.md` document into a typed [`Feedback`].
///
/// Tolerant: a missing `status:` line is treated as `pending`, an absent
/// frontmatter fence still yields a body-only pending item.
fn parse(run: &str, id: &str, raw: &str) -> Feedback {
    let mut status = FeedbackStatus::Pending;
    let mut station = String::new();
    let mut severity = None;
    let mut created_at = None;

    // Split frontmatter (between the first two `---` fences) from the body.
    let (front, body) = split_frontmatter(raw);
    for line in front.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("status:") {
            if let Some(s) = parse_status(rest) {
                status = s;
            }
        } else if let Some(rest) = line.strip_prefix("station:") {
            station = rest.trim().trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("severity:") {
            severity = parse_severity(rest);
        } else if let Some(rest) = line.strip_prefix("created_at:") {
            let v = rest.trim().trim_matches('"').to_string();
            if !v.is_empty() {
                created_at = Some(v);
            }
        }
    }

    Feedback {
        id: id.to_string(),
        run: run.to_string(),
        station,
        status,
        severity,
        body: body.trim_start_matches('\n').to_string(),
        created_at,
    }
}

/// Split a frontmatter document into `(frontmatter, body)`. When there is no
/// leading `---` fence the whole input is the body.
fn split_frontmatter(raw: &str) -> (String, String) {
    let trimmed = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    if let Some(rest) = trimmed.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let front = rest[..end].to_string();
            let after = &rest[end + 4..];
            let body = after.strip_prefix('\n').unwrap_or(after).to_string();
            return (front, body);
        }
    }
    (String::new(), trimmed.to_string())
}

/// List every feedback item for a run, parsed and sorted by id.
pub fn list(store: &StateStore, run: &str) -> Result<Vec<Feedback>> {
    let raw = store.read_feedback_raw(run)?;
    Ok(raw
        .into_iter()
        .map(|(id, content)| parse(run, &id, &content))
        .collect())
}

/// Read a single feedback item by id.
pub fn get(store: &StateStore, run: &str, id: &str) -> Result<Feedback> {
    let raw = store.read_feedback_raw(run)?;
    raw.get(id)
        .map(|content| parse(run, id, content))
        .ok_or_else(|| McpError::FeedbackNotFound(id.to_string()))
}

/// Allocate the next free `fb-NN` id for a run.
fn next_id(store: &StateStore, run: &str) -> Result<String> {
    let raw = store.read_feedback_raw(run)?;
    let mut max = 0u32;
    for id in raw.keys() {
        if let Some(num) = id.strip_prefix("fb-") {
            if let Ok(n) = num.parse::<u32>() {
                max = max.max(n);
            }
        }
    }
    Ok(format!("fb-{:02}", max + 1))
}

/// Create a new feedback item, returning the persisted record.
pub fn create(
    store: &StateStore,
    run: &str,
    station: &str,
    body: &str,
    severity: Option<FeedbackSeverity>,
) -> Result<Feedback> {
    if body.trim().is_empty() {
        return Err(McpError::InvalidInput("feedback body must not be empty".into()));
    }
    let id = next_id(store, run)?;
    let fb = Feedback {
        id: id.clone(),
        run: run.to_string(),
        station: station.to_string(),
        status: FeedbackStatus::Pending,
        severity,
        body: body.to_string(),
        created_at: Some(Utc::now().to_rfc3339()),
    };
    store.write_feedback_raw(run, &id, &serialize(&fb))?;
    Ok(fb)
}

/// Transition a feedback item to a new status, returning the updated record.
///
/// Settled (terminal) items are immutable — attempting to re-transition one
/// errors. This mirrors the predecessor's "closed/rejected are immutable"
/// rule and keeps the manager's open-feedback walk monotone.
pub fn set_status(
    store: &StateStore,
    run: &str,
    id: &str,
    status: FeedbackStatus,
) -> Result<Feedback> {
    let mut fb = get(store, run, id)?;
    if is_terminal(fb.status) {
        return Err(McpError::FeedbackSettled(id.to_string()));
    }
    fb.status = status;
    store.write_feedback_raw(run, id, &serialize(&fb))?;
    Ok(fb)
}

/// Reject a feedback item: stamp it `rejected` and append the reason to the
/// body. Rejection is a terminal transition, so the manager stops
/// re-dispatching it on the next tick.
pub fn reject(store: &StateStore, run: &str, id: &str, reason: &str) -> Result<Feedback> {
    let mut fb = get(store, run, id)?;
    if is_terminal(fb.status) {
        return Err(McpError::FeedbackSettled(id.to_string()));
    }
    fb.status = FeedbackStatus::Rejected;
    if !reason.trim().is_empty() {
        if !fb.body.ends_with('\n') {
            fb.body.push('\n');
        }
        fb.body.push_str(&format!("\n---\nRejected: {reason}\n"));
    }
    store.write_feedback_raw(run, id, &serialize(&fb))?;
    Ok(fb)
}

/// Set (or re-target) the station a feedback item belongs to — triage
/// placement. Settled items are immutable.
pub fn move_station(store: &StateStore, run: &str, id: &str, to_station: &str) -> Result<Feedback> {
    let mut fb = get(store, run, id)?;
    if is_terminal(fb.status) {
        return Err(McpError::FeedbackSettled(id.to_string()));
    }
    fb.station = to_station.to_string();
    store.write_feedback_raw(run, id, &serialize(&fb))?;
    Ok(fb)
}

/// Set the severity ranking of a feedback item. Settled items are immutable.
pub fn set_severity(
    store: &StateStore,
    run: &str,
    id: &str,
    severity: FeedbackSeverity,
) -> Result<Feedback> {
    let mut fb = get(store, run, id)?;
    if is_terminal(fb.status) {
        return Err(McpError::FeedbackSettled(id.to_string()));
    }
    fb.severity = Some(severity);
    store.write_feedback_raw(run, id, &serialize(&fb))?;
    Ok(fb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn store() -> (tempfile::TempDir, StateStore) {
        let dir = tempdir().expect("tmp");
        let store = StateStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn create_allocates_sequential_ids() {
        let (_d, store) = store();
        let a = create(&store, "r", "frame", "first", None).unwrap();
        let b = create(&store, "r", "frame", "second", None).unwrap();
        assert_eq!(a.id, "fb-01");
        assert_eq!(b.id, "fb-02");
        assert_eq!(a.status, FeedbackStatus::Pending);
    }

    #[test]
    fn create_rejects_empty_body() {
        let (_d, store) = store();
        let err = create(&store, "r", "frame", "   ", None).unwrap_err();
        assert!(matches!(err, McpError::InvalidInput(_)));
    }

    #[test]
    fn roundtrip_preserves_fields() {
        let (_d, store) = store();
        let made = create(
            &store,
            "r",
            "build",
            "the widget overflows",
            Some(FeedbackSeverity::High),
        )
        .unwrap();
        let read = get(&store, "r", &made.id).unwrap();
        assert_eq!(read.station, "build");
        assert_eq!(read.severity, Some(FeedbackSeverity::High));
        assert_eq!(read.body.trim(), "the widget overflows");
        assert!(read.created_at.is_some());
    }

    #[test]
    fn set_status_advances_and_blocks_terminal() {
        let (_d, store) = store();
        let fb = create(&store, "r", "frame", "x", None).unwrap();
        let fixing = set_status(&store, "r", &fb.id, FeedbackStatus::Fixing).unwrap();
        assert_eq!(fixing.status, FeedbackStatus::Fixing);
        let closed = set_status(&store, "r", &fb.id, FeedbackStatus::Addressed).unwrap();
        assert_eq!(closed.status, FeedbackStatus::Addressed);
        // Now terminal → further transitions error.
        let err = set_status(&store, "r", &fb.id, FeedbackStatus::Pending).unwrap_err();
        assert!(matches!(err, McpError::FeedbackSettled(_)));
    }

    #[test]
    fn reject_is_terminal_and_appends_reason() {
        let (_d, store) = store();
        let fb = create(&store, "r", "frame", "bad", None).unwrap();
        let rejected = reject(&store, "r", &fb.id, "stale duplicate").unwrap();
        assert_eq!(rejected.status, FeedbackStatus::Rejected);
        assert!(rejected.body.contains("stale duplicate"));
        assert!(is_terminal(rejected.status));
    }

    #[test]
    fn move_relocates_station() {
        let (_d, store) = store();
        let fb = create(&store, "r", "frame", "x", None).unwrap();
        let moved = move_station(&store, "r", &fb.id, "shape").unwrap();
        assert_eq!(moved.station, "shape");
    }

    #[test]
    fn set_severity_updates_rank() {
        let (_d, store) = store();
        let fb = create(&store, "r", "frame", "x", None).unwrap();
        let ranked = set_severity(&store, "r", &fb.id, FeedbackSeverity::Blocker).unwrap();
        assert_eq!(ranked.severity, Some(FeedbackSeverity::Blocker));
    }

    #[test]
    fn get_missing_errors() {
        let (_d, store) = store();
        let err = get(&store, "r", "fb-99").unwrap_err();
        assert!(matches!(err, McpError::FeedbackNotFound(_)));
    }

    #[test]
    fn list_returns_all_sorted() {
        let (_d, store) = store();
        create(&store, "r", "frame", "a", None).unwrap();
        create(&store, "r", "frame", "b", None).unwrap();
        let all = list(&store, "r").unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, "fb-01");
        assert_eq!(all[1].id, "fb-02");
    }
}
