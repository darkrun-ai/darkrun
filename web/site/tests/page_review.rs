//! Integration tests for `status_tone` — the /review page's mapping from a
//! display status string onto a UI tone.

use darkrun_site::pages::review::status_tone;
use darkrun_ui::prelude::Tone;

#[test]
fn ok_statuses_map_to_ok() {
    for s in ["approved", "locked", "done", "passed"] {
        assert_eq!(status_tone(s), Tone::Ok, "{s}");
    }
}

#[test]
fn danger_statuses_map_to_danger() {
    for s in ["blocked", "failed", "rejected"] {
        assert_eq!(status_tone(s), Tone::Danger, "{s}");
    }
}

#[test]
fn info_statuses_map_to_info() {
    for s in ["in_review", "review", "active"] {
        assert_eq!(status_tone(s), Tone::Info, "{s}");
    }
}

#[test]
fn warn_statuses_map_to_warn() {
    for s in ["pending", "queued"] {
        assert_eq!(status_tone(s), Tone::Warn, "{s}");
    }
}

#[test]
fn unknown_status_falls_back_to_neutral() {
    for s in ["", "unknown", "wat", "Approved", "DONE", "in review"] {
        assert_eq!(status_tone(s), Tone::Neutral, "{s}");
    }
}

#[test]
fn mapping_is_case_sensitive() {
    // Wire statuses are lowercase; uppercase variants are not recognized.
    assert_eq!(status_tone("APPROVED"), Tone::Neutral);
    assert_eq!(status_tone("Blocked"), Tone::Neutral);
    assert_ne!(status_tone("approved"), status_tone("Approved"));
}

#[test]
fn the_four_tone_buckets_are_distinct() {
    assert_ne!(status_tone("done"), status_tone("failed"));
    assert_ne!(status_tone("failed"), status_tone("active"));
    assert_ne!(status_tone("active"), status_tone("pending"));
    assert_ne!(status_tone("pending"), status_tone("done"));
}

#[test]
fn mapping_is_deterministic() {
    for s in ["approved", "blocked", "active", "pending", "xyz"] {
        assert_eq!(status_tone(s), status_tone(s));
    }
}

#[test]
fn whitespace_is_not_trimmed() {
    // Leading/trailing whitespace is part of the key, so it falls through.
    assert_eq!(status_tone(" approved"), Tone::Neutral);
    assert_eq!(status_tone("approved "), Tone::Neutral);
}

#[test]
fn rejected_is_danger_not_neutral() {
    // `rejected` belongs to the danger bucket even though it sounds terminal.
    assert_eq!(status_tone("rejected"), Tone::Danger);
}
