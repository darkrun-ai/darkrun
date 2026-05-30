//! Run-level helpers: list summaries and archive toggling.
//!
//! These back the `darkrun_run_list` / `darkrun_run_archive` tools. Listing
//! returns a compact summary per run (slug, title, factory, status, active
//! station, archived flag) without forcing the caller to read every document.

use darkrun_core::domain::Status;
use darkrun_core::StateStore;
use serde::Serialize;

use crate::error::{McpError, Result};

/// A compact summary of a run for list views.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RunSummary {
    /// Run slug.
    pub slug: String,
    /// Resolved title.
    pub title: String,
    /// Driving factory.
    pub factory: String,
    /// Lifecycle status.
    pub status: Status,
    /// The active station (write-cache hint).
    pub active_station: String,
    /// Whether this run is archived.
    pub archived: bool,
}

/// List every run on disk as a summary, sorted by slug. Archived runs are
/// included unless `include_archived` is false.
pub fn list(store: &StateStore, include_archived: bool) -> Result<Vec<RunSummary>> {
    let mut out = Vec::new();
    for slug in store.list_runs()? {
        let run = match store.read_run(&slug) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let archived = run.frontmatter.archived.unwrap_or(false);
        if archived && !include_archived {
            continue;
        }
        out.push(RunSummary {
            slug: run.slug,
            title: run.title,
            factory: run.frontmatter.factory,
            status: run.frontmatter.status,
            active_station: run.frontmatter.active_station,
            archived,
        });
    }
    Ok(out)
}

/// Set (or clear) a run's archived flag. Archiving a run also clears it from
/// the active-run pointer so it stops surfacing as the default.
pub fn set_archived(store: &StateStore, slug: &str, archived: bool) -> Result<()> {
    let mut run = store
        .read_run(slug)
        .map_err(|_| McpError::Core(darkrun_core::CoreError::RunNotFound(slug.to_string())))?;
    run.frontmatter.archived = Some(archived);
    store.write_run(&run)?;
    if archived {
        // If this run was the active pointer, drop it.
        if let Ok(Some(active)) = store.active_run() {
            if active == slug {
                store.clear_active_run()?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::run_start;
    use tempfile::tempdir;

    fn store() -> (tempfile::TempDir, StateStore) {
        let dir = tempdir().expect("tmp");
        let store = StateStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn list_returns_summaries() {
        let (_d, store) = store();
        run_start(&store, "a", "software", Some("Alpha".into()), "continuous").unwrap();
        run_start(&store, "b", "software", None, "continuous").unwrap();
        let runs = list(&store, true).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].slug, "a");
        assert_eq!(runs[0].title, "Alpha");
        assert_eq!(runs[0].active_station, "frame");
    }

    #[test]
    fn archive_hides_run_from_default_list() {
        let (_d, store) = store();
        run_start(&store, "a", "software", None, "continuous").unwrap();
        set_archived(&store, "a", true).unwrap();
        assert!(list(&store, false).unwrap().is_empty());
        assert_eq!(list(&store, true).unwrap().len(), 1);
        assert!(list(&store, true).unwrap()[0].archived);
    }

    #[test]
    fn archive_clears_active_pointer() {
        let (_d, store) = store();
        run_start(&store, "a", "software", None, "continuous").unwrap();
        store.set_active_run("a").unwrap();
        set_archived(&store, "a", true).unwrap();
        // Active should no longer resolve to the archived run.
        assert_ne!(store.active_run().unwrap(), Some("a".to_string()));
    }

    #[test]
    fn unarchive_restores() {
        let (_d, store) = store();
        run_start(&store, "a", "software", None, "continuous").unwrap();
        set_archived(&store, "a", true).unwrap();
        set_archived(&store, "a", false).unwrap();
        assert_eq!(list(&store, false).unwrap().len(), 1);
    }

    #[test]
    fn archive_missing_run_errors() {
        let (_d, store) = store();
        let err = set_archived(&store, "ghost", true).unwrap_err();
        assert!(matches!(err, McpError::Core(_)));
    }
}
