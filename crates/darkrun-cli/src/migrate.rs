//! `darkrun migrate` — carry legacy `.darkrun/` run state forward to the
//! current on-disk schema shape.
//!
//! A run's `state.json` records the on-disk STATE-SHAPE version it was written
//! in ([`darkrun_core::SCHEMA_VERSION`]). The engine migrates old shapes forward
//! *on read* (a pure, non-persisting step), so a legacy run keeps working — but
//! its file stays at the old shape on disk until the next write. This command
//! makes that migration explicit and durable: it reports what each run needs
//! (the default **dry-run**), and with `--apply` reads the run through the
//! engine's migrator and writes the migrated shape back.
//!
//! The dry-run/apply split, the one-slug-at-a-time default (vs `--all`), and the
//! clean-tree guard (`--allow-dirty` to override) mirror the documented
//! `darkrun-migrate` command + skill so the plugin surface is reachable.

use std::path::Path;

use darkrun_core::{StateStore, SCHEMA_VERSION, SCHEMA_VERSION_LEGACY};
use darkrun_git::{Git, GitBackend};

type BoxError = Box<dyn std::error::Error>;

/// The schema migration a single run's on-disk `state.json` needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunMigration {
    /// The run slug.
    pub slug: String,
    /// The on-disk schema version the file currently records.
    pub from_version: u32,
    /// The schema version this build migrates to ([`SCHEMA_VERSION`]).
    pub to_version: u32,
}

impl RunMigration {
    /// True when the file is already at (or past) the current shape — nothing to do.
    pub fn is_up_to_date(&self) -> bool {
        self.from_version >= self.to_version
    }
}

/// Inspect a run's on-disk `state.json` and report the schema migration it needs.
/// `Ok(None)` when the run has no `state.json` (nothing to migrate).
pub fn plan_run(store: &StateStore, slug: &str) -> Result<Option<RunMigration>, BoxError> {
    let path = store.run_dir(slug).join("state.json");
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    let from = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(SCHEMA_VERSION_LEGACY as u64) as u32;
    Ok(Some(RunMigration {
        slug: slug.to_string(),
        from_version: from,
        to_version: SCHEMA_VERSION,
    }))
}

/// Apply the migration for one run: read it through the engine's on-read
/// migrator (which forwards the shape in memory) and write the result back,
/// persisting the current schema version.
pub fn apply_run(store: &StateStore, slug: &str) -> Result<(), BoxError> {
    if let Some(state) = store.read_state(slug)? {
        store.write_state(slug, &state)?;
    }
    Ok(())
}

/// Arguments for `darkrun migrate`.
#[derive(Debug)]
pub struct MigrateArgs {
    /// The run slug to migrate (required unless `--all`).
    pub slug: Option<String>,
    /// Write the migration instead of only reporting it (default: dry-run).
    pub apply: bool,
    /// Migrate every run in the store rather than a single slug.
    pub all: bool,
    /// Permit `--apply` even when the working tree has uncommitted changes.
    pub allow_dirty: bool,
}

/// Handle `darkrun migrate`.
pub fn migrate_command(repo_root: &Path, args: MigrateArgs) -> Result<(), BoxError> {
    let store = StateStore::new(repo_root);

    let slugs: Vec<String> = if args.all {
        if args.slug.is_some() {
            return Err("pass a slug OR --all, not both".into());
        }
        store.list_runs()?
    } else {
        match args.slug {
            Some(slug) => vec![slug],
            None => {
                return Err("pass a run slug (or --all) — e.g. `darkrun migrate <slug>`".into())
            }
        }
    };

    if slugs.is_empty() {
        println!("No runs found under {}", store.root().display());
        return Ok(());
    }

    // Guard the apply against a dirty tree so the rewrite is reviewable/rollback-
    // able. A non-git checkout has nothing to guard, so a failed open is benign.
    if args.apply && !args.allow_dirty {
        if let Ok(git) = Git::open(repo_root) {
            if !git.is_clean()? {
                return Err("working tree has uncommitted changes — commit first, \
                            or pass --allow-dirty to migrate anyway"
                    .into());
            }
        }
    }

    let mut migrated = 0usize;
    for slug in &slugs {
        match plan_run(&store, slug)? {
            None => println!("{slug}: no state.json — nothing to migrate"),
            Some(plan) if plan.is_up_to_date() => {
                println!("{slug}: already at schema v{} — up to date", plan.to_version)
            }
            Some(plan) => {
                if args.apply {
                    apply_run(&store, slug)?;
                    println!(
                        "{slug}: migrated schema v{} → v{}",
                        plan.from_version, plan.to_version
                    );
                    migrated += 1;
                } else {
                    println!(
                        "{slug}: would migrate schema v{} → v{}",
                        plan.from_version, plan.to_version
                    );
                }
            }
        }
    }

    if args.apply {
        println!("\nMigrated {migrated} run(s).");
    } else {
        println!("\nDry-run only — re-run with --apply to write the changes.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkrun_core::RunState;

    fn temp_store() -> (tempfile::TempDir, StateStore) {
        let dir = tempfile::tempdir().expect("tmp");
        let store = StateStore::new(dir.path());
        (dir, store)
    }

    /// Write a raw `state.json` for a run, bypassing the typed writer so we can
    /// model a legacy doc that omits `schema_version`.
    fn write_raw_state(store: &StateStore, slug: &str, json: &str) {
        let dir = store.run_dir(slug);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("state.json"), json).unwrap();
    }

    #[test]
    fn plan_none_when_no_state_file() {
        let (_d, store) = temp_store();
        assert!(plan_run(&store, "ghost").unwrap().is_none());
    }

    #[test]
    fn plan_reports_legacy_version_for_unstamped_state() {
        let (_d, store) = temp_store();
        // A legacy doc: an active_station but no schema_version.
        write_raw_state(&store, "old", r#"{"active_station":"Frame"}"#);
        let plan = plan_run(&store, "old").unwrap().unwrap();
        assert_eq!(plan.from_version, SCHEMA_VERSION_LEGACY);
        assert_eq!(plan.to_version, SCHEMA_VERSION);
        assert!(!plan.is_up_to_date());
    }

    #[test]
    fn plan_is_up_to_date_at_current_version() {
        let (_d, store) = temp_store();
        // A doc already stamped at the current schema version needs no migration.
        let state = RunState {
            schema_version: Some(SCHEMA_VERSION),
            ..RunState::default()
        };
        store.write_state("cur", &state).unwrap();
        let plan = plan_run(&store, "cur").unwrap().unwrap();
        assert_eq!(plan.from_version, SCHEMA_VERSION);
        assert!(plan.is_up_to_date());
    }

    #[test]
    fn apply_stamps_the_current_schema_version_on_disk() {
        let (_d, store) = temp_store();
        write_raw_state(&store, "old", r#"{"active_station":"Frame"}"#);
        // Before: no schema_version on disk.
        assert_eq!(plan_run(&store, "old").unwrap().unwrap().from_version, 0);

        apply_run(&store, "old").unwrap();

        // After: the file carries the current schema version, so a re-plan is a
        // no-op.
        let after = plan_run(&store, "old").unwrap().unwrap();
        assert_eq!(after.from_version, SCHEMA_VERSION);
        assert!(after.is_up_to_date());
    }

    #[test]
    fn apply_is_a_noop_for_a_missing_state_file() {
        let (_d, store) = temp_store();
        // No panic, no file created.
        apply_run(&store, "ghost").unwrap();
        assert!(plan_run(&store, "ghost").unwrap().is_none());
    }
}
