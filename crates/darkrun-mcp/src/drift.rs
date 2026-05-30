//! Drift track (Track C) scaffold.
//!
//! Drift is a witnessed artifact mutation — a locked artifact whose on-disk
//! content no longer matches the hash the engine recorded. It preempts both
//! feedback and run work because building on a silently-changed artifact
//! produces inconsistent output.
//!
//! The drift *sweep* (hashing every locked artifact and diffing) is a
//! `darkrun-core` concern that is not yet wired (see `deferred`). Until it
//! lands, this module reads any drift entries an external sweep has already
//! deposited under `.darkrun/<run>/drift/*.md`, so the manager's Track C can
//! surface them. With no entries the track is a no-op and control falls
//! through to feedback.

use std::fs;

use darkrun_core::domain::{Drift, DriftKind};
use darkrun_core::StateStore;

use crate::error::Result;

/// The `drift/` directory for a run.
fn drift_dir(store: &StateStore, run: &str) -> std::path::PathBuf {
    store.run_dir(run).join("drift")
}

/// Parse a drift kind, defaulting to `Output`.
fn parse_kind(raw: &str) -> DriftKind {
    match raw.trim().trim_matches('"').to_ascii_lowercase().as_str() {
        "spec" => DriftKind::Spec,
        "discovery_output" => DriftKind::DiscoveryOutput,
        "discovery_mandate" => DriftKind::DiscoveryMandate,
        _ => DriftKind::Output,
    }
}

/// Parse one raw `drift/*.md` document into a [`Drift`].
fn parse(run: &str, raw: &str) -> Drift {
    let mut path = String::new();
    let mut station = String::new();
    let mut kind = DriftKind::Output;
    let mut age = String::new();
    let mut unit = None;
    for line in raw.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("path:") {
            path = rest.trim().trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("station:") {
            station = rest.trim().trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("kind:") {
            kind = parse_kind(rest);
        } else if let Some(rest) = line.strip_prefix("age:") {
            age = rest.trim().trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("unit:") {
            let v = rest.trim().trim_matches('"').to_string();
            if !v.is_empty() {
                unit = Some(v);
            }
        }
    }
    Drift {
        path,
        station,
        run: run.to_string(),
        kind,
        age,
        unit,
    }
}

/// Read every drift entry for a run (sorted by file stem). Empty when no
/// sweep has deposited entries.
pub fn list(store: &StateStore, run: &str) -> Result<Vec<Drift>> {
    let dir = drift_dir(store, run);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<(String, Drift)> = Vec::new();
    for entry in fs::read_dir(&dir).map_err(darkrun_core::CoreError::from)? {
        let entry = entry.map_err(darkrun_core::CoreError::from)?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let raw = fs::read_to_string(&path).map_err(darkrun_core::CoreError::from)?;
                entries.push((stem.to_string(), parse(run, &raw)));
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries.into_iter().map(|(_, d)| d).collect())
}

/// The first (highest-priority) drift entry for a run, if any. The manager
/// uses this to drive Track C.
pub fn first(store: &StateStore, run: &str) -> Result<Option<Drift>> {
    Ok(list(store, run)?.into_iter().next())
}

/// Record a drift entry under `drift/<id>.md`. Used by tests and by the
/// (future) core sweep until it owns drift storage natively.
pub fn record(store: &StateStore, run: &str, id: &str, entry: &Drift) -> Result<()> {
    let dir = drift_dir(store, run);
    fs::create_dir_all(&dir).map_err(|source| darkrun_core::CoreError::Io {
        path: dir.clone(),
        source,
    })?;
    let kind = match entry.kind {
        DriftKind::Spec => "spec",
        DriftKind::Output => "output",
        DriftKind::DiscoveryOutput => "discovery_output",
        DriftKind::DiscoveryMandate => "discovery_mandate",
    };
    let mut doc = String::from("---\n");
    doc.push_str(&format!("path: {}\n", entry.path));
    doc.push_str(&format!("station: {}\n", entry.station));
    doc.push_str(&format!("kind: {kind}\n"));
    if !entry.age.is_empty() {
        doc.push_str(&format!("age: {}\n", entry.age));
    }
    if let Some(unit) = &entry.unit {
        doc.push_str(&format!("unit: {unit}\n"));
    }
    doc.push_str("---\n");
    let path = dir.join(format!("{id}.md"));
    fs::write(&path, doc).map_err(|source| {
        darkrun_core::CoreError::Io {
            path: path.clone(),
            source,
        }
        .into()
    })
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
    fn empty_when_no_dir() {
        let (_d, store) = store();
        assert!(list(&store, "r").unwrap().is_empty());
        assert!(first(&store, "r").unwrap().is_none());
    }

    #[test]
    fn record_and_read_back() {
        let (_d, store) = store();
        let d = Drift {
            path: "frame/frame.md".into(),
            station: "frame".into(),
            run: "r".into(),
            kind: DriftKind::Spec,
            age: "5m".into(),
            unit: Some("u1".into()),
        };
        record(&store, "r", "d-01", &d).unwrap();
        let read = list(&store, "r").unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].path, "frame/frame.md");
        assert_eq!(read[0].kind, DriftKind::Spec);
        assert_eq!(read[0].unit, Some("u1".to_string()));
        assert_eq!(first(&store, "r").unwrap().unwrap().station, "frame");
    }
}
