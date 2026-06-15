//! Project-level knowledge — the explorer-maintained shared memory.
//!
//! Discovery explorers surface durable project facts (constraints, prior art,
//! traps, conventions). Unlike a run's per-station discovery, these are
//! **project-scoped**: they persist in `.darkrun/knowledge/<topic>.md` across
//! runs, so a later run's Spec reads them as priors instead of re-discovering
//! the same ground. Keyed by `topic` slug — re-recording a topic updates the
//! prior in place (the predecessor's `scope: project` knowledge that decompose
//! reads and updates when it diverges).

use chrono::Utc;
use darkrun_core::StateStore;
use serde::Serialize;

use crate::error::Result;

/// A piece of durable project knowledge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Knowledge {
    /// The topic slug (the doc id).
    pub topic: String,
    /// When the topic was first recorded.
    pub created_at: String,
    /// When the topic was last updated.
    pub updated_at: String,
    /// The knowledge prose.
    pub body: String,
    /// The SHA-256 of the document on disk — the concurrency token. A
    /// subsequent overwrite of this topic must present this `sha` as
    /// `expected_sha`, so a re-record can't silently clobber a newer edit.
    pub sha: String,
}

/// Sanitize a topic into a single safe path component.
fn safe_topic(topic: &str) -> String {
    let s: String = topic
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    if s.is_empty() { "untitled".to_string() } else { s }
}

/// Record project knowledge for `topic`, updating it in place when the topic
/// already exists (preserving its original `created_at`).
///
/// Overwriting an EXISTING topic is GUARDED: `expected_sha` must match the
/// topic's current on-disk sha (read it first via [`list`]/[`get`]). A missing
/// or stale sha returns [`CoreError::Conflict`](darkrun_core::CoreError::Conflict)
/// rather than clobbering a newer edit. Recording a NEW topic needs no sha.
pub fn record(
    store: &StateStore,
    topic: &str,
    body: &str,
    expected_sha: Option<&str>,
) -> Result<Knowledge> {
    let topic = safe_topic(topic);
    let now = Utc::now().to_rfc3339();
    // Preserve the original created_at when updating an existing topic.
    let created_at = store
        .read_knowledge_entry(&topic)?
        .map(|doc| parse(topic.clone(), &doc, String::new()).created_at)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| now.clone());
    let doc = format!(
        "---\ntopic: {topic}\ncreated_at: {created_at}\nupdated_at: {now}\n---\n{}\n",
        body.trim()
    );
    let sha = store.write_knowledge_guarded(&topic, &doc, expected_sha)?;
    let _ = crate::commit::commit_state(store, &format!("darkrun: knowledge {topic}"));
    Ok(Knowledge {
        topic,
        created_at,
        updated_at: now,
        body: body.trim().to_string(),
        sha,
    })
}

/// List every project knowledge entry, by topic, each with its concurrency sha.
pub fn list(store: &StateStore) -> Result<Vec<Knowledge>> {
    let raw = store.read_knowledge_raw()?;
    Ok(raw
        .into_iter()
        .map(|(id, doc)| {
            let sha = darkrun_core::hash_bytes(doc.as_bytes());
            parse(id, &doc, sha)
        })
        .collect())
}

/// Read one knowledge topic with its concurrency sha, or `None` if absent.
pub fn get(store: &StateStore, topic: &str) -> Result<Option<Knowledge>> {
    let topic = safe_topic(topic);
    Ok(store
        .read_knowledge_with_sha(&topic)?
        .map(|(doc, sha)| parse(topic, &doc, sha)))
}

/// Parse a knowledge document. Tolerant of a missing frontmatter fence.
fn parse(topic: String, doc: &str, sha: String) -> Knowledge {
    let doc = doc.trim_start_matches('\u{feff}');
    let field = |fm: &str, key: &str| -> String {
        fm.lines()
            .find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim().trim_matches('"').to_string()))
            .unwrap_or_default()
    };
    if let Some(rest) = doc.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let fm = &rest[..end];
            let body = rest[end + 4..].trim_start_matches('\n').trim().to_string();
            return Knowledge {
                topic,
                created_at: field(fm, "created_at:"),
                updated_at: field(fm, "updated_at:"),
                body,
                sha,
            };
        }
    }
    Knowledge {
        topic,
        created_at: String::new(),
        updated_at: String::new(),
        body: doc.trim().to_string(),
        sha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn store() -> (tempfile::TempDir, StateStore) {
        let dir = tempdir().unwrap();
        let store = StateStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn record_is_project_scoped_and_lists() {
        let (_d, store) = store();
        record(&store, "auth-conventions", "use the shared CredentialStore", None).unwrap();
        record(&store, "build-traps", "the wasm target needs --no-default-features", None).unwrap();
        let all = list(&store).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|k| k.topic == "auth-conventions" && k.body.contains("CredentialStore")));
        // Every listed entry carries a concurrency sha.
        assert!(all.iter().all(|k| k.sha.len() == 64));
        // Stored at the PROJECT root, not under any run.
        assert!(store.knowledge_dir().join("auth-conventions.md").exists());
    }

    #[test]
    fn re_recording_a_topic_requires_the_current_sha() {
        let (_d, store) = store();
        let first = record(&store, "x", "v1", None).unwrap();
        // A bare re-record (no sha) of an existing topic is REFUSED.
        let blind = record(&store, "x", "v2", None);
        assert!(
            matches!(blind, Err(crate::error::McpError::Core(darkrun_core::CoreError::Conflict(_)))),
            "overwriting without a sha must conflict"
        );
        // Presenting the current sha succeeds and preserves created_at.
        let second = record(&store, "x", "v2 revised", Some(&first.sha)).unwrap();
        let all = list(&store).unwrap();
        assert_eq!(all.len(), 1, "same topic overwrites");
        assert_eq!(all[0].body, "v2 revised");
        assert_eq!(second.created_at, first.created_at, "created_at preserved across updates");
        // A STALE sha (the first one) now conflicts.
        let stale = record(&store, "x", "v3", Some(&first.sha));
        assert!(matches!(
            stale,
            Err(crate::error::McpError::Core(darkrun_core::CoreError::Conflict(_)))
        ));
    }

    #[test]
    fn get_returns_the_topic_with_its_sha() {
        let (_d, store) = store();
        assert!(get(&store, "missing").unwrap().is_none());
        let rec = record(&store, "y", "body", None).unwrap();
        let got = get(&store, "y").unwrap().unwrap();
        assert_eq!(got.body, "body");
        assert_eq!(got.sha, rec.sha);
    }

    #[test]
    fn topic_is_sanitized() {
        let (_d, store) = store();
        let k = record(&store, "a/b c!", "x", None).unwrap();
        assert_eq!(k.topic, "a-b-c-");
        assert!(store.knowledge_dir().join("a-b-c-.md").exists());
    }
}
