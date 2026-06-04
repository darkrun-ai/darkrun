//! Drift track (Track C) scaffold.
//!
//! Drift is a witnessed artifact mutation — a locked artifact whose on-disk
//! content no longer matches the hash the engine recorded. It preempts both
//! feedback and run work because building on a silently-changed artifact
//! produces inconsistent output.
//!
//! The drift *sweep* — [`record_station_witnesses`] snapshots a station's
//! locked artifacts (a content hash per output) when it completes, and
//! [`sweep`] re-hashes every witness each tick: a hash that no longer matches
//! (or a vanished file) deposits a drift entry, and a hash that matches again
//! clears a stale one (so reverting an artifact self-heals). [`accept`]
//! re-witnesses an intentional change. The manager's Track C reads the deposited
//! entries via [`first`]; with none, the track is a no-op.

use std::fs;
use std::path::PathBuf;

use darkrun_core::domain::{Drift, DriftKind};
use darkrun_core::{hash_file, StateStore, Witness};

use crate::error::Result;

/// The repo root the run's artifact paths are relative to — the parent of the
/// `.darkrun/` state root.
fn repo_root(store: &StateStore) -> PathBuf {
    store
        .root()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| store.root().to_path_buf())
}

/// A filename-safe drift id derived from an artifact path (so re-sweeps of the
/// same artifact overwrite rather than pile up).
pub(crate) fn drift_id_for(path: &str) -> String {
    let mut id = String::from("drift-");
    for c in path.chars() {
        id.push(if c.is_ascii_alphanumeric() { c } else { '_' });
    }
    id
}

/// Remove a drift entry if present (idempotent).
fn clear(store: &StateStore, run: &str, id: &str) -> Result<()> {
    let path = drift_dir(store, run).join(format!("{id}.md"));
    if path.exists() {
        fs::remove_file(&path).map_err(darkrun_core::CoreError::from)?;
    }
    Ok(())
}

/// The cap on open drift entries before the sweep stops filing new ones — a
/// circuit breaker so a cascade (a widely-consumed artifact moving, or a
/// directory rename) can't bury the run under a flood of drift the operator
/// can't act on. Overridable via `DARKRUN_DRIFT_CASCADE_CAP`.
fn cascade_cap() -> usize {
    std::env::var("DARKRUN_DRIFT_CASCADE_CAP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10)
}

/// Snapshot the premises of a just-completed station: hash every **output** of
/// its completed units into the shared [`Witness`] store, AND every declared
/// **input** into each unit's `input_witnesses` (the premise the unit was built
/// on). Called when a station locks. A later sweep flags an output that mutated
/// *or* an input premise that moved upstream.
pub fn record_station_witnesses(store: &StateStore, run: &str, station: &str) -> Result<()> {
    let root = repo_root(store);
    let units = store.read_units(run)?;
    let mut witnesses = store.read_witnesses(run)?;
    for u in units.iter().filter(|u| {
        u.station() == station && matches!(u.status(), darkrun_core::domain::Status::Completed)
    }) {
        for out in &u.frontmatter.outputs {
            if let Some(hash) = hash_file(&root.join(out)) {
                witnesses.retain(|w| w.path != *out);
                witnesses.push(Witness {
                    path: out.clone(),
                    hash,
                    station: station.to_string(),
                    unit: Some(u.slug.clone()),
                });
            }
        }
    }
    store.write_witnesses(run, &witnesses)?;

    // Per-unit input premises: hash each declared input and stamp it onto the
    // unit so a later upstream change to that input surfaces as Input drift.
    for u in units.iter().filter(|u| {
        u.station() == station && matches!(u.status(), darkrun_core::domain::Status::Completed)
    }) {
        let mut unit = u.clone();
        let mut changed = false;
        for input in &u.frontmatter.inputs {
            if let Some(hash) = hash_file(&root.join(input)) {
                unit.frontmatter.input_witnesses.insert(input.clone(), hash);
                changed = true;
            }
        }
        if changed {
            store.write_unit(run, &unit)?;
        }
    }
    Ok(())
}

/// Re-hash every witness; deposit a drift entry for any artifact whose content
/// changed or vanished, and clear the entry for any that matches again. Pure
/// over disk — same files, same result. Run at the top of each tick.
pub fn sweep(store: &StateStore, run: &str) -> Result<()> {
    let root = repo_root(store);
    let cap = cascade_cap();
    // Count currently-open drift so the cascade breaker can stop filing once the
    // run is already flooded (clears still run — a flood can still drain).
    let mut open = list(store, run)?.len();

    // 1. Output witnesses (shared store).
    for w in store.read_witnesses(run)? {
        let id = drift_id_for(&w.path);
        let drifted = match hash_file(&root.join(&w.path)) {
            Some(current) => current != w.hash,
            None => true, // a missing locked artifact is drift
        };
        if drifted {
            if file_exists(store, run, &id) || open < cap {
                let entry = Drift {
                    path: w.path.clone(),
                    station: w.station.clone(),
                    run: run.to_string(),
                    kind: DriftKind::Output,
                    age: String::new(),
                    unit: w.unit.clone(),
                };
                if !file_exists(store, run, &id) {
                    open += 1;
                }
                record(store, run, &id, &entry)?;
            }
        } else {
            clear(store, run, &id)?;
        }
    }

    // 2. Input premises (per-unit `input_witnesses`). An upstream artifact a
    //    completed unit was signed against has moved → the unit rests on a stale
    //    premise. The drift id keys on (unit, input) so it's idempotent per
    //    premise and never collides with the output-drift id for the same path.
    for unit in store.read_units(run)? {
        for (path, witnessed) in &unit.frontmatter.input_witnesses {
            let id = format!("{}__in_{}", unit.slug, drift_id_for(path));
            let drifted = match hash_file(&root.join(path)) {
                Some(current) => current != *witnessed,
                None => true,
            };
            if drifted {
                if file_exists(store, run, &id) || open < cap {
                    let entry = Drift {
                        path: path.clone(),
                        station: unit.station().to_string(),
                        run: run.to_string(),
                        kind: DriftKind::Input,
                        age: String::new(),
                        unit: Some(unit.slug.clone()),
                    };
                    if !file_exists(store, run, &id) {
                        open += 1;
                    }
                    record(store, run, &id, &entry)?;
                }
            } else {
                clear(store, run, &id)?;
            }
        }
    }
    Ok(())
}

/// Whether a drift entry with `id` is already on disk.
fn file_exists(store: &StateStore, run: &str, id: &str) -> bool {
    drift_dir(store, run).join(format!("{id}.md")).exists()
}

/// Accept an intentional change to a locked artifact: re-witness it to its
/// current content hash, re-anchor every annotation pinned to it against the new
/// version (text re-anchors; image/pdf regions re-crop), and clear the drift
/// entry. Returns `false` if the path isn't witnessed or the file is unreadable.
/// (The other resolution — reverting the artifact — needs no tool: the next
/// [`sweep`] clears the drift on its own.)
pub fn accept(store: &StateStore, run: &str, path: &str) -> Result<bool> {
    let root = repo_root(store);
    let full = root.join(path);
    let Some(hash) = hash_file(&full) else {
        return Ok(false);
    };
    // B9: capture the drift's effective station up front (its own, else the
    // active station — mirroring how the cursor's ResolveDrift resolved it) so
    // that once the drift resolves we can land its fix worktree back onto the
    // station branch. Best-effort + idempotent downstream.
    let fix_station = list(store, run)?
        .into_iter()
        .find(|d| d.path == path && !d.station.is_empty())
        .map(|d| d.station)
        .or_else(|| {
            store
                .read_state(run)
                .ok()
                .flatten()
                .map(|s| s.active_station)
        })
        .filter(|s| !s.is_empty());
    let mut witnesses = store.read_witnesses(run)?;
    let mut found = false;
    for w in witnesses.iter_mut() {
        if w.path == path {
            w.hash = hash.clone();
            found = true;
        }
    }

    // Re-witness the same path as an INPUT premise on any unit that consumed it,
    // clearing that unit's input-drift entry too. An intentional upstream change
    // is accepted everywhere it was a premise, not just where it was an output.
    for mut unit in store.read_units(run)? {
        if unit.frontmatter.input_witnesses.contains_key(path) {
            unit.frontmatter.input_witnesses.insert(path.to_string(), hash.clone());
            store.write_unit(run, &unit)?;
            clear(store, run, &format!("{}__in_{}", unit.slug, drift_id_for(path)))?;
            found = true;
        }
    }

    if found {
        store.write_witnesses(run, &witnesses)?;
        clear(store, run, &drift_id_for(path))?;
        // Refresh the annotations on this artifact against the new version in
        // one pass: text spans re-anchor, image/pdf region crops are re-cut from
        // the new bytes, and a region that no longer frames the same content is
        // flagged rather than silently mis-cropped.
        if let Ok(bytes) = fs::read(&full) {
            crate::annotation::reanchor_artifact_version(store, &root, run, path, &bytes)?;
        }
        // The drift is resolved — land its fix worktree (if any) back onto the
        // station branch and retire it. Idempotent: a drift resolved without a
        // forked fix worktree (non-git run) no-ops.
        if let Some(station) = &fix_station {
            crate::lifecycle::land_fix(store, run, station, &drift_id_for(path));
        }
    }
    Ok(found)
}

/// The `drift/` directory for a run.
fn drift_dir(store: &StateStore, run: &str) -> std::path::PathBuf {
    store.run_dir(run).join("drift")
}

/// Parse a drift kind, defaulting to `Output`.
fn parse_kind(raw: &str) -> DriftKind {
    match raw.trim().trim_matches('"').to_ascii_lowercase().as_str() {
        "spec" => DriftKind::Spec,
        "input" => DriftKind::Input,
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
        DriftKind::Input => "input",
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

    use darkrun_core::domain::{Status, Unit, UnitFrontmatter};

    /// A store whose `.darkrun` root sits under `repo`, so the sweep's
    /// `repo_root` resolves back to `repo` where the witnessed artifacts live.
    /// (`StateStore::new` appends `.darkrun` itself.)
    fn repo_store() -> (tempfile::TempDir, StateStore, std::path::PathBuf) {
        let dir = tempdir().expect("tmp");
        let repo = dir.path().to_path_buf();
        let store = StateStore::new(&repo);
        (dir, store, repo)
    }

    fn completed_unit_with_output(station: &str, slug: &str, output: &str) -> Unit {
        Unit {
            slug: slug.into(),
            frontmatter: UnitFrontmatter {
                status: Status::Completed,
                station: Some(station.into()),
                outputs: vec![output.into()],
                ..Default::default()
            },
            title: slug.into(),
            body: String::new(),
        }
    }

    #[test]
    fn sweep_detects_mutation_then_self_heals_on_revert() {
        let (_d, store, repo) = repo_store();
        store
            .write_unit("r", &completed_unit_with_output("frame", "u1", "out.txt"))
            .unwrap();
        fs::write(repo.join("out.txt"), b"v1").unwrap();
        record_station_witnesses(&store, "r", "frame").unwrap();
        assert_eq!(store.read_witnesses("r").unwrap().len(), 1);

        // Clean: no drift.
        sweep(&store, "r").unwrap();
        assert!(first(&store, "r").unwrap().is_none());

        // Mutated: drift on that artifact.
        fs::write(repo.join("out.txt"), b"v2").unwrap();
        sweep(&store, "r").unwrap();
        let d = first(&store, "r").unwrap().expect("drift");
        assert_eq!(d.path, "out.txt");
        assert_eq!(d.station, "frame");

        // Reverted: the sweep clears the drift on its own.
        fs::write(repo.join("out.txt"), b"v1").unwrap();
        sweep(&store, "r").unwrap();
        assert!(first(&store, "r").unwrap().is_none());

        // Vanished locked artifact is itself drift.
        fs::remove_file(repo.join("out.txt")).unwrap();
        sweep(&store, "r").unwrap();
        assert!(first(&store, "r").unwrap().is_some());
    }

    #[test]
    fn sweep_detects_input_premise_drift_and_accept_rewitnesses_it() {
        let (_d, store, repo) = repo_store();
        // An upstream artifact this unit consumed as an input premise.
        fs::write(repo.join("spec.md"), b"spec-v1").unwrap();
        let mut unit = completed_unit_with_output("build", "u1", "impl.rs");
        unit.frontmatter.inputs = vec!["spec.md".into()];
        fs::write(repo.join("impl.rs"), b"code").unwrap();
        store.write_unit("r", &unit).unwrap();
        record_station_witnesses(&store, "r", "build").unwrap();

        // The input premise is witnessed onto the unit.
        let back = store.read_unit("r", "u1").unwrap();
        assert_eq!(back.frontmatter.input_witnesses.get("spec.md").cloned(),
                   Some(darkrun_core::hash_bytes(b"spec-v1")));

        // Clean sweep: no drift.
        sweep(&store, "r").unwrap();
        assert!(first(&store, "r").unwrap().is_none());

        // The upstream input moves → Input drift on this unit.
        fs::write(repo.join("spec.md"), b"spec-v2").unwrap();
        sweep(&store, "r").unwrap();
        let d = first(&store, "r").unwrap().expect("input drift");
        assert_eq!(d.kind, DriftKind::Input);
        assert_eq!(d.path, "spec.md");
        assert_eq!(d.unit, Some("u1".to_string()));

        // Accept re-witnesses the premise; the drift clears.
        assert!(accept(&store, "r", "spec.md").unwrap());
        sweep(&store, "r").unwrap();
        assert!(first(&store, "r").unwrap().is_none());
    }

    #[test]
    fn cascade_breaker_caps_open_drift() {
        let (_d, store, repo) = repo_store();
        std::env::set_var("DARKRUN_DRIFT_CASCADE_CAP", "3");
        // Six witnessed outputs, all about to mutate.
        for i in 0..6 {
            let out = format!("o{i}.txt");
            store
                .write_unit("r", &completed_unit_with_output("frame", &format!("u{i}"), &out))
                .unwrap();
            fs::write(repo.join(&out), b"v1").unwrap();
        }
        record_station_witnesses(&store, "r", "frame").unwrap();
        for i in 0..6 {
            fs::write(repo.join(format!("o{i}.txt")), b"v2").unwrap();
        }
        sweep(&store, "r").unwrap();
        // The breaker caps new filings at the configured cap.
        assert_eq!(list(&store, "r").unwrap().len(), 3);
        std::env::remove_var("DARKRUN_DRIFT_CASCADE_CAP");
    }

    #[test]
    fn accept_rewitnesses_an_intentional_change() {
        let (_d, store, repo) = repo_store();
        store
            .write_unit("r", &completed_unit_with_output("frame", "u1", "out.txt"))
            .unwrap();
        fs::write(repo.join("out.txt"), b"v1").unwrap();
        record_station_witnesses(&store, "r", "frame").unwrap();

        fs::write(repo.join("out.txt"), b"v2").unwrap();
        sweep(&store, "r").unwrap();
        assert!(first(&store, "r").unwrap().is_some());

        // Accept the new content → drift clears, witness updates to v2.
        assert!(accept(&store, "r", "out.txt").unwrap());
        sweep(&store, "r").unwrap();
        assert!(first(&store, "r").unwrap().is_none());
        assert_eq!(
            store.read_witnesses("r").unwrap()[0].hash,
            darkrun_core::hash_bytes(b"v2")
        );

        // Accepting an unknown path is a no-op false.
        assert!(!accept(&store, "r", "nope.txt").unwrap());
    }
}
