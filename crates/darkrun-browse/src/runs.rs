//! Project a repo's on-disk `.darkrun/` run state into the [`darkrun_api`]
//! browse payloads, reading straight off [`darkrun_core::StateStore`] with no
//! engine and no HTTP server.
//!
//! - [`list_runs`] — every non-archived run as a [`RunSummary`], sorted by slug,
//!   wrapped in a [`RunListPayload`].
//! - [`run_detail`] — a single run's [`RunDetailPayload`]: identity, live
//!   position, every station it walks, and the units on the active station.
//!   `None` when the run is unknown.
//!
//! Display strings (`status`, `phase`) come from the domain enums' serde
//! representation, so they stay in lockstep with the wire contract without a
//! hand-maintained match. The HTTP server and the desktop's offline view both
//! call these, so every surface agrees.

use darkrun_api::{
    FeedbackOrigin, RunDetailPayload, RunDetailStation, RunDetailUnit, RunListPayload, RunSummary,
    StationProgress,
};
use darkrun_core::domain::{Run, Station, StationPhase, Status, Unit};
use darkrun_core::state::RunState;
use darkrun_core::StateStore;
use std::path::Path as FsPath;

use crate::feedback_doc::FeedbackDoc;

/// The run's stable branch (`darkrun/<slug>/main`) — MUST mirror the engine's
/// `lifecycle::run_main_branch` naming (station work lands on
/// `darkrun/<slug>/<station>` and merges into this). Checking a branch that
/// doesn't exist made `authored_by_me` false for every run, so the desktop's
/// default "Mine" filter hid them all.
fn run_branch(slug: &str) -> String {
    format!("darkrun/{slug}/main")
}

/// The base branch a run forks from — `default_branch` out of
/// `.darkrun/settings.yml`, defaulting to `main` when unset or unreadable.
///
/// Parsed line-wise (the file is the flat `key: value` document
/// `darkrun_setup` writes) so this stays free of a YAML dependency.
fn base_branch(darkrun_root: &FsPath) -> String {
    let raw = std::fs::read_to_string(darkrun_root.join("settings.yml")).unwrap_or_default();
    for line in raw.lines() {
        if let Some(value) = line.trim().strip_prefix("default_branch:") {
            let value = value.trim().trim_matches(['"', '\'']).trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    "main".to_string()
}

/// Resolves the "Mine" predicate for run branches against one repository,
/// resolving the current git identity once up front so the per-run check is a
/// single revwalk.
struct Authorship {
    /// The repository root (the parent of `.darkrun`).
    repo_root: std::path::PathBuf,
    /// The base branch every run forks from.
    base: String,
    /// The effective `user.email`, lowercased — `None` when no identity is
    /// configured or the project is not a git repo (then nothing is mine).
    email: Option<String>,
}

impl Authorship {
    /// Build the resolver from the state store's `.darkrun` root.
    fn resolve(darkrun_root: &FsPath) -> Self {
        // repo_root is the parent of `.darkrun`; fall back to the root itself.
        let repo_root = darkrun_root.parent().unwrap_or(darkrun_root).to_path_buf();
        let email = darkrun_git::current_identity_email(&repo_root)
            .ok()
            .flatten()
            .map(|e| e.to_ascii_lowercase());
        Authorship {
            repo_root,
            base: base_branch(darkrun_root),
            email,
        }
    }

    /// Whether the current identity authored any commit on the run's branch.
    /// `false` when there is no configured identity to match.
    fn mine(&self, slug: &str) -> bool {
        let Some(email) = self.email.as_deref() else {
            return false;
        };
        darkrun_git::branch_authored_by(&self.repo_root, &self.base, &run_branch(slug), email)
            .unwrap_or(false)
    }

    /// The run branch's author NAME (the run owner), for display + author search.
    fn author(&self, slug: &str) -> Option<String> {
        darkrun_git::branch_author(&self.repo_root, &self.base, &run_branch(slug))
            .ok()
            .flatten()
    }
}

/// Render a `serde`-enum value (e.g. [`Status`]) to its wire string. Falls back
/// to an empty string if the value did not serialize to a bare JSON string —
/// which the domain enums never do.
fn wire_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The display string for a station phase.
fn phase_string(phase: StationPhase) -> String {
    wire_string(&phase)
}

/// Compute station progress (completed / total) from a run's derived state.
/// `None` when no state has been written yet — the run is counted as having no
/// stations.
fn progress_from_state(state: Option<&RunState>) -> StationProgress {
    let Some(state) = state else {
        return StationProgress::default();
    };
    let total = state.stations.len() as u32;
    let completed = state
        .stations
        .values()
        .filter(|s| s.status == Status::Completed)
        .count() as u32;
    StationProgress { completed, total }
}

/// The active station's phase, if the run's state records one for it.
fn active_phase(run: &Run, state: Option<&RunState>) -> Option<String> {
    let station = &run.frontmatter.active_station;
    state
        .and_then(|s| s.stations.get(station))
        .map(|s| phase_string(s.phase))
}

/// Count a run's OPEN drift feedback: `origin = drift` items still holding a
/// gate. The drift sweep files these when a locked input premise moves, so the
/// count is readable straight off the run's feedback sidecars (no engine
/// dependency), and the number is exactly what the operator would find in the
/// inbox.
fn open_drift(store: &StateStore, slug: &str) -> u32 {
    store
        .read_feedback_raw(slug)
        .unwrap_or_default()
        .into_iter()
        .map(|(id, content)| FeedbackDoc::parse(&id, &content))
        // `is_open`, not `blocks_gate`: an ESCALATED drift item still needs the
        // operator, so the chip must count it (the engine counts it against the
        // drift cascade too). Only terminal resolutions drop off.
        .filter(|doc| doc.origin == FeedbackOrigin::Drift && doc.status.is_open())
        .count() as u32
}

/// Project a [`Run`] (+ its derived state, if present) into a [`RunSummary`].
///
/// `authored_by_me` is the engine's "Mine" predicate for this run's branch; the
/// caller resolves it once via [`Authorship`] and threads it in so the
/// projection stays a pure function.
fn summarize(
    run: &Run,
    state: Option<&RunState>,
    authored_by_me: bool,
    author: Option<String>,
    open_drift: u32,
) -> RunSummary {
    RunSummary {
        slug: run.slug.clone(),
        title: run.title.clone(),
        factory: run.frontmatter.factory.clone(),
        active_station: run.frontmatter.active_station.clone(),
        phase: active_phase(run, state),
        status: wire_string(&run.frontmatter.status),
        progress: progress_from_state(state),
        started_at: run.frontmatter.started_at.clone(),
        authored_by_me,
        author,
        open_drift,
    }
}

/// Walk order for a run's stations: by `started_at` (stamped before unstamped),
/// then by name. A station that has started sorts ahead of one that hasn't; two
/// unstarted (or two same-time) stations fall back to name order.
fn station_walk_order(a: &RunDetailStation, b: &RunDetailStation) -> std::cmp::Ordering {
    match (&a.started_at, &b.started_at) {
        (Some(x), Some(y)) => x.cmp(y).then_with(|| a.name.cmp(&b.name)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.name.cmp(&b.name),
    }
}

/// Project a single derived [`Station`] into a detail row.
fn detail_station(station: &Station) -> RunDetailStation {
    RunDetailStation {
        name: station.station.clone(),
        status: wire_string(&station.status),
        phase: Some(phase_string(station.phase)),
        started_at: station.started_at.clone(),
        completed_at: station.completed_at.clone(),
    }
}

/// Project a [`Unit`] into a detail row.
fn detail_unit(unit: &Unit) -> RunDetailUnit {
    RunDetailUnit {
        slug: unit.slug.clone(),
        title: unit.title.clone(),
        status: wire_string(&unit.frontmatter.status),
        station: unit.frontmatter.station.clone(),
    }
}

/// List the project's runs as summaries, sorted by slug.
///
/// Archived runs are omitted (mirroring the engine's default list view). Runs
/// whose document fails to parse are skipped rather than failing the whole
/// list, so one corrupt sidecar never blanks the browse view.
pub fn list_runs(store: &StateStore) -> RunListPayload {
    let mut summaries = Vec::new();

    // Resolve the current git identity + base branch once for the whole list so
    // the per-run "Mine" check is a single revwalk.
    let authorship = Authorship::resolve(store.root());

    if let Ok(slugs) = store.list_runs() {
        for slug in slugs {
            let Ok(run) = store.read_run(&slug) else {
                continue;
            };
            if run.frontmatter.archived.unwrap_or(false) {
                continue;
            }
            let run_state = store.read_state(&slug).ok().flatten();
            let mine = authorship.mine(&slug);
            let author = authorship.author(&slug);
            let drift = open_drift(store, &slug);
            summaries.push(summarize(&run, run_state.as_ref(), mine, author, drift));
        }
    }

    // `list_runs` already returns slugs sorted, but re-sort defensively so the
    // wire order is a guaranteed property regardless of the store's ordering.
    summaries.sort_by(|a, b| a.slug.cmp(&b.slug));

    RunListPayload::new(summaries)
}

/// A single run's detail: identity, live position, every station it walks, and
/// the units on the active station. `None` when no such run exists.
pub fn run_detail(store: &StateStore, slug: &str) -> Option<RunDetailPayload> {
    let run = store.read_run(slug).ok()?;
    let state = store.read_state(slug).ok().flatten();

    // Stations in walk order (by `started_at`), with each station's lifecycle
    // status derived through the SHARED `darkrun_core::derive::station_status`
    // (index-relative to the active station) — the same path the engine wire
    // payload and the desktop use — so every surface agrees. The active station
    // keeps its recorded status so a `Blocked` nuance isn't lost.
    let mut stations: Vec<RunDetailStation> = state
        .as_ref()
        .map(|s| s.stations.values().map(detail_station).collect())
        .unwrap_or_default();
    stations.sort_by(station_walk_order);
    let active_index = stations
        .iter()
        .position(|s| s.name == run.frontmatter.active_station);
    for (i, st) in stations.iter_mut().enumerate() {
        if let Status::Active = darkrun_core::derive::station_status(i, active_index) {
            // active station: keep its recorded status string
        } else {
            st.status = wire_string(&darkrun_core::derive::station_status(i, active_index));
        }
    }

    // Units on the active station only.
    let active = &run.frontmatter.active_station;
    let mut units: Vec<RunDetailUnit> = store
        .read_units(slug)
        .unwrap_or_default()
        .iter()
        .filter(|u| u.station() == active)
        .map(detail_unit)
        .collect();
    units.sort_by(|a, b| a.slug.cmp(&b.slug));

    Some(RunDetailPayload {
        slug: run.slug.clone(),
        title: run.title.clone(),
        factory: run.frontmatter.factory.clone(),
        active_station: run.frontmatter.active_station.clone(),
        phase: active_phase(&run, state.as_ref()),
        status: wire_string(&run.frontmatter.status),
        progress: progress_from_state(state.as_ref()),
        started_at: run.frontmatter.started_at.clone(),
        stations,
        units,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn station_walk_order_covers_every_started_at_arm() {
        use std::cmp::Ordering;
        let row = |name: &str, started: Option<&str>| RunDetailStation {
            name: name.into(),
            status: "pending".into(),
            phase: None,
            started_at: started.map(str::to_string),
            completed_at: None,
        };
        let stamped_a = row("a", Some("2026-05-30T00:00:00Z"));
        let stamped_b = row("b", Some("2026-05-30T01:00:00Z"));
        let unstamped_a = row("a", None);
        let unstamped_z = row("z", None);
        // Both stamped → by time, then name.
        assert_eq!(station_walk_order(&stamped_a, &stamped_b), Ordering::Less);
        // Stamped before unstamped, and the reverse.
        assert_eq!(station_walk_order(&stamped_a, &unstamped_z), Ordering::Less);
        assert_eq!(station_walk_order(&unstamped_z, &stamped_a), Ordering::Greater);
        // Both unstamped → by name.
        assert_eq!(station_walk_order(&unstamped_a, &unstamped_z), Ordering::Less);
    }

    #[test]
    fn base_branch_reads_default_branch_or_falls_back_to_main() {
        let dir = tempfile::tempdir().unwrap();
        // No settings file → the `main` default.
        assert_eq!(base_branch(dir.path()), "main");
        // A `default_branch:` line is read (quotes + whitespace trimmed).
        std::fs::write(
            dir.path().join("settings.yml"),
            "default_branch: \"develop\"\nhosting: github\n",
        )
        .unwrap();
        assert_eq!(base_branch(dir.path()), "develop");
        // An empty value falls back to main.
        std::fs::write(dir.path().join("settings.yml"), "default_branch:   \n").unwrap();
        assert_eq!(base_branch(dir.path()), "main");
    }

    #[test]
    fn list_and_detail_read_a_run_off_disk_with_no_engine() {
        use darkrun_core::domain::{Run, RunFrontmatter};
        let repo = tempfile::tempdir().unwrap();
        let store = StateStore::new(repo.path());
        let run = Run {
            slug: "widget-1".into(),
            title: "Widget".into(),
            frontmatter: RunFrontmatter {
                // Title is resolved from frontmatter on the write→read roundtrip,
                // so it must live here, not only on the `Run.title` field.
                title: Some("Widget".into()),
                factory: "app".into(),
                active_station: "frame".into(),
                status: Status::Active,
                ..Default::default()
            },
            body: String::new(),
        };
        store.write_run(&run).unwrap();

        let list = list_runs(&store);
        assert_eq!(list.count, 1);
        assert_eq!(list.runs[0].slug, "widget-1");
        assert_eq!(list.runs[0].title, "Widget");

        let detail = run_detail(&store, "widget-1").expect("run detail");
        assert_eq!(detail.slug, "widget-1");
        assert_eq!(detail.active_station, "frame");
        // An unknown run yields None, not an error.
        assert!(run_detail(&store, "no-such-run").is_none());
    }

    #[test]
    fn archived_runs_drop_out_of_the_list() {
        use darkrun_core::domain::{Run, RunFrontmatter};
        let repo = tempfile::tempdir().unwrap();
        let store = StateStore::new(repo.path());
        let run = Run {
            slug: "old-1".into(),
            title: "Old".into(),
            frontmatter: RunFrontmatter {
                factory: "app".into(),
                active_station: "frame".into(),
                status: Status::Completed,
                archived: Some(true),
                ..Default::default()
            },
            body: String::new(),
        };
        store.write_run(&run).unwrap();
        assert_eq!(list_runs(&store).count, 0, "archived run is hidden");
        // ...but it is still directly addressable by detail.
        assert!(run_detail(&store, "old-1").is_some());
    }
}
