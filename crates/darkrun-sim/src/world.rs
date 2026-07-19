//! The world driver: a **real** dark-mode engine Run in a bare `tempfile`
//! tempdir, driven tick-by-tick from the prompt-blind [`Provider`] decisions,
//! with target resolution done from the world's OWN per-tick `StateStore` reads.
//!
//! This is the fidelity boundary the frame names: nothing here mocks the
//! engine. The world owns a real [`StateStore`], starts a Run in `Mode::Dark`
//! via [`run_start`], and advances it only through
//! [`run_tick_with_hosting`] against the sim's [`NoopHosting`] — never the
//! network-reaching `run_tick`. The driving loop reads a tick's `.prompt` (the
//! rendered instruction text), hands it to the provider, executes the returned
//! move against the state-resolved target, and re-ticks.
//!
//! The one place the structured action variant is read is the private
//! [`grade_tick`] free function below — the post-hoc grading path. No decision
//! in the tick loop branches on it (AC-3): the provider's move is a function of
//! its own private state, and the move's TARGET is resolved from `read_state` /
//! `read_units`, never from the tick's structured action.

use std::collections::BTreeSet;

use darkrun_core::domain::{Mode, Status, Unit, UnitFrontmatter};
use darkrun_core::StateStore;
use darkrun_mcp::hosting::{Hosting, MergeState, OpenRequest};
use darkrun_mcp::position::{
    checkpoint_decide, elaborate_seal, run_review_stamp, run_start, run_tick_with_hosting,
    RunAction, TickResult,
};

use crate::provider::{Provider, ProviderMove};

/// A no-op [`Hosting`] client: performs zero I/O and satisfies the trait's three
/// non-defaulted methods with literal values. Vendored here because
/// `darkrun-mcp` exports no public stub (its only `Stub` lives in a private test
/// module). A `run_tick_with_hosting<H: Hosting>` needs a complete `Hosting`
/// impl to compile, exercised or not.
pub struct NoopHosting;

impl Hosting for NoopHosting {
    fn available(&self) -> bool {
        false
    }
    fn open_draft(&self, _req: &OpenRequest) -> Option<String> {
        None
    }
    fn merge_state(&self, _pr_ref: &str) -> MergeState {
        MergeState::Unknown
    }
}

/// The sim's entire red/green verdict vocabulary for a driven run — the
/// in-memory twin of the fixture's `FixtureOutcome`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldOutcome {
    /// The manager's cursor reached `Sealed` — protocol flowed.
    Sealed,
    /// The engine's deadlock guard fired `Escalate` — a no-progress wedge.
    Escalated {
        /// The engine's own escalation diagnostic.
        reason: String,
    },
}

/// Everything a driven run yields: the per-tick prompt capture (in ORDER, one
/// entry per `run_tick_with_hosting` call), the move sequence the provider
/// emitted, the terminal outcome, and every verifier nonce that was live during
/// the run (the transcript projector normalizes these out).
#[derive(Debug, Clone)]
pub struct DriveResult {
    /// Each tick's `TickResult.prompt`, captured the moment the tick returned
    /// it — never re-read from the overwrite-on-reuse on-disk prompt files.
    pub prompts: Vec<Option<String>>,
    /// The provider's emitted moves, in order (excludes the never-reached tail).
    pub moves: Vec<ProviderMove>,
    /// The terminal outcome the grading path observed.
    pub outcome: WorldOutcome,
    /// Every distinct verifier nonce minted during the run (collected live,
    /// before `complete_station` retires each to `None`).
    pub nonces: BTreeSet<String>,
}

/// A self-contained dark-mode Run: a bare tempdir + the store rooted in it + the
/// run slug. Dropping it tears down all on-disk state.
pub struct World {
    _dir: tempfile::TempDir,
    /// The real on-disk state store backing the Run.
    pub store: StateStore,
    /// The Run slug.
    pub slug: String,
}

/// The safety ceiling on a drive loop — well above any real scenario's tick
/// count, so a genuine non-convergence fails loudly instead of hanging.
const DRIVE_GUARD: usize = 2000;

impl World {
    /// Start a fresh dark-mode Run of `factory` in a bare `tempfile` tempdir
    /// (no `.git`, no pre-seeded state), passing the `Mode::Dark` enum variant
    /// DIRECTLY to `run_start` — never a string through `Mode::from_label`, so a
    /// typo can't silently downgrade the mode. Its own tempdir + `StateStore`
    /// are distinct from `harness.rs`'s linter partition.
    pub fn new(slug: &str, factory: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = StateStore::new(dir.path());
        // The world is its own guard against a double `run_start` for a slug
        // this store already carries (the engine itself overwrites silently).
        assert_fresh_start(&store, slug);
        run_start(&store, slug, factory, None, Mode::Dark, "full").expect("run_start");
        World {
            _dir: dir,
            store,
            slug: slug.to_string(),
        }
    }

    /// Drive the Run to a terminal outcome, consulting `provider` for each
    /// non-terminal tick's move.
    pub fn drive<P: Provider>(&self, provider: &mut P) -> DriveResult {
        self.drive_inner(provider, false)
    }

    /// [`drive`](Self::drive), but corrupting every NON-identifier state field (a
    /// unit's body/title, the run's title) after each tick while leaving every
    /// slug/station/status/dependency untouched. Test support for
    /// `state_reads_resolve_targets_only`: the move sequence must be unchanged,
    /// proving target resolution reads only identifiers.
    pub fn drive_corrupting<P: Provider>(&self, provider: &mut P) -> DriveResult {
        self.drive_inner(provider, true)
    }

    fn drive_inner<P: Provider>(&self, provider: &mut P, corrupt: bool) -> DriveResult {
        let mut prompts: Vec<Option<String>> = Vec::new();
        let mut moves: Vec<ProviderMove> = Vec::new();
        let mut nonces: BTreeSet<String> = BTreeSet::new();
        let mut guard = 0usize;
        loop {
            guard += 1;
            assert!(
                guard < DRIVE_GUARD,
                "world drive failed to converge after {guard} ticks — no terminal outcome"
            );

            let tick = self.tick();
            prompts.push(tick.prompt.clone());
            self.collect_live_nonces(&mut nonces);

            // Post-hoc grading. Its result feeds ONLY the stop condition and the
            // outcome, never which move comes next; the loop breaks on the FIRST
            // terminal observation, so a sealed/escalated run is never ticked
            // past operationally.
            if let Some(outcome) = grade_tick(&tick) {
                return DriveResult {
                    prompts,
                    moves,
                    outcome,
                    nonces,
                };
            }

            let mv = provider.next_move(tick.prompt.as_deref());
            moves.push(mv);
            match mv {
                ProviderMove::Stop => panic!(
                    "provider returned Stop after {} moves with no terminal outcome observed — \
                     the scripted sequence is exhausted (harness failure, not a followability finding)",
                    moves.len()
                ),
                ProviderMove::AdvanceStation => self.execute_advance_station(),
                ProviderMove::CompleteWave => self.execute_complete_wave(),
                ProviderMove::Approve => self.execute_approve(),
                ProviderMove::StampRunReviewers => self.execute_stamp_run_reviewers(),
            }

            if corrupt {
                self.corrupt_non_identifier_state();
            }
        }
    }

    /// One tick via `run_tick_with_hosting` against [`NoopHosting`] — the only
    /// engine-advance entry point this module uses.
    fn tick(&self) -> TickResult {
        run_tick_with_hosting(&self.store, &self.slug, &NoopHosting).expect("run_tick_with_hosting")
    }

    /// The recorded active station (a per-tick `StateStore` read), or `None`
    /// once the run has no active station recorded.
    fn active_station(&self) -> Option<String> {
        self.store
            .read_state(&self.slug)
            .ok()
            .flatten()
            .map(|s| s.active_station)
            .filter(|s| !s.is_empty())
    }

    /// Execute `AdvanceStation`: seed the resolved active station's wave (decompose
    /// one unit if it owes none) and clear its elaboration hold. Idempotent for a
    /// station that already owes a wave. A rejection from `elaborate_seal` (the
    /// station is not active) is a driver-bookkeeping bug, never a protocol
    /// finding, so it surfaces loudly rather than being swallowed.
    fn execute_advance_station(&self) {
        let Some(station) = self.active_station() else {
            return;
        };
        let unit = format!("{station}-unit");
        if self.store.read_unit(&self.slug, &unit).is_err() {
            self.decompose_one(&station, &unit);
        }
        elaborate_seal(&self.store, &self.slug, &station)
            .expect("elaborate_seal on the resolved active station");
    }

    /// Execute `CompleteWave`: mark every not-yet-completed unit on the resolved
    /// active station completed.
    fn execute_complete_wave(&self) {
        let Some(station) = self.active_station() else {
            return;
        };
        for u in self.store.read_units(&self.slug).unwrap_or_default() {
            if u.station() == station && !matches!(u.status(), Status::Completed) {
                let mut u = u;
                u.frontmatter.status = Status::Completed;
                self.store.write_unit(&self.slug, &u).expect("write_unit");
            }
        }
    }

    /// Execute `Approve`: decide the current hold with no feedback. Used only by
    /// non-dark scenarios; a rejection (no gate open) is a driver bug and panics.
    fn execute_approve(&self) {
        checkpoint_decide(&self.store, &self.slug, true, None).expect("checkpoint_decide approve");
    }

    /// Execute `StampRunReviewers`: stamp every still-unsigned run-level reviewer.
    /// Dark mode declares none, so this resolves to an empty set and no-ops.
    fn execute_stamp_run_reviewers(&self) {
        for role in self.unsigned_run_reviewers() {
            run_review_stamp(&self.store, &self.slug, &role).expect("run_review_stamp");
        }
    }

    /// Decompose one wave unit on `station`, copying the station's declared
    /// inputs so the runtime input-coverage gate is satisfied. Direct
    /// `store.write_unit` per Contract 4's module map.
    fn decompose_one(&self, station: &str, unit_slug: &str) {
        let inputs = self
            .store
            .read_run(&self.slug)
            .ok()
            .and_then(|r| darkrun_mcp::resolve_factory(&r.frontmatter.factory))
            .and_then(|f| f.station(station).map(|d| d.inputs.clone()))
            .unwrap_or_default();
        let unit = Unit {
            slug: unit_slug.to_string(),
            frontmatter: UnitFrontmatter {
                status: Status::Pending,
                station: Some(station.to_string()),
                inputs,
                ..Default::default()
            },
            title: unit_slug.to_string(),
            body: String::new(),
        };
        self.store.write_unit(&self.slug, &unit).expect("write_unit");
    }

    /// The run-level reviewers the factory declares that state has not yet
    /// stamped. Empty in dark mode (the engine declares no run reviewers there).
    fn unsigned_run_reviewers(&self) -> Vec<String> {
        let factory = self
            .store
            .read_run(&self.slug)
            .ok()
            .and_then(|r| darkrun_mcp::resolve_factory(&r.frontmatter.factory));
        let state = self.store.read_state(&self.slug).ok().flatten();
        let (Some(factory), Some(state)) = (factory, state) else {
            return Vec::new();
        };
        factory
            .run_reviewers
            .iter()
            .filter(|r| !matches!(state.run_reviews.get(*r), Some(Some(_))))
            .cloned()
            .collect()
    }

    /// Accumulate every verifier nonce currently live in state (before
    /// `complete_station` retires each to `None`). A per-tick `StateStore` read.
    fn collect_live_nonces(&self, nonces: &mut BTreeSet<String>) {
        if let Ok(Some(state)) = self.store.read_state(&self.slug) {
            for st in state.stations.values() {
                if let Some(n) = &st.verifier_nonce {
                    nonces.insert(n.clone());
                }
            }
        }
    }

    /// Corrupt only NON-identifier fields — the run's title/body and each unit's
    /// title/body. Every slug, station assignment, status, and dependency is
    /// left intact, so target resolution (which reads only identifiers) sees an
    /// unchanged world.
    fn corrupt_non_identifier_state(&self) {
        if let Ok(mut run) = self.store.read_run(&self.slug) {
            run.frontmatter.title = Some("corrupted-title-xyzzy".to_string());
            run.body = "corrupted run body — not an identifier".to_string();
            let _ = self.store.write_run(&run);
        }
        for u in self.store.read_units(&self.slug).unwrap_or_default() {
            let mut u = u;
            u.frontmatter.name = Some("corrupted-unit-name".to_string());
            u.body = "corrupted unit body — not an identifier".to_string();
            let _ = self.store.write_unit(&self.slug, &u);
        }
    }
}

/// Observe the terminal outcome of a tick by reading its structured action —
/// the ONE place this module reads the structured action variant, and the whole
/// of AC-3's grading confinement. Its result feeds ONLY the drive loop's stop
/// condition and the emitted `FixtureOutcome`; it NEVER selects which
/// `ProviderMove` comes next. A gate extracts this body by name, so it is a
/// free fn at column zero with its closing brace on its own line.
fn grade_tick(tick: &TickResult) -> Option<WorldOutcome> {
    match &tick.action {
        RunAction::Sealed { .. } => Some(WorldOutcome::Sealed),
        RunAction::Escalate { reason, .. } => Some(WorldOutcome::Escalated {
            reason: reason.clone(),
        }),
        _ => None,
    }
}

/// The world's own double-`run_start` guard: a Run must not be started twice for
/// a slug this store already carries (the engine overwrites silently otherwise).
fn assert_fresh_start(store: &StateStore, slug: &str) {
    assert!(
        store.read_run(slug).is_err(),
        "double run_start guard: a Run for slug `{slug}` already exists in this store — \
         run_start must run exactly once per World"
    );
}

/// The scripted move sequence that drives a dark-mode `software` run to `Sealed`.
///
/// Per station the engine walks six recorded phases (Spec → Review → Manufacture
/// → Audit → Reflect → Checkpoint(auto)); the world's tick-first loop consults
/// the provider once per non-terminal tick. The uniform six-move block below
/// makes forward progress where the tick needs it (decompose at Spec, complete
/// at Manufacture) and is a harmless idempotent re-seal on the auto phases; the
/// Checkpoint move seeds the NEXT station the auto-completion just entered. The
/// trailing `Stop` is a safety net — the loop breaks on `Sealed` before it is
/// ever reached when the accounting holds.
pub fn dark_core_script() -> Vec<ProviderMove> {
    let stations = darkrun_mcp::resolve_factory("software")
        .map(|f| f.stations.len())
        .unwrap_or(6);
    let per_station = [
        ProviderMove::AdvanceStation, // Spec: decompose the wave + seal
        ProviderMove::AdvanceStation, // Review: idempotent re-seal
        ProviderMove::CompleteWave,   // Manufacture: complete the wave
        ProviderMove::AdvanceStation, // Audit: idempotent
        ProviderMove::AdvanceStation, // Reflect: idempotent
        ProviderMove::AdvanceStation, // Checkpoint(auto): seed the next station
    ];
    let mut moves = Vec::with_capacity(stations * per_station.len() + 1);
    for _ in 0..stations {
        moves.extend_from_slice(&per_station);
    }
    moves.push(ProviderMove::Stop);
    moves
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ScriptedProvider;

    /// AC-9 (named): an induced no-progress loop is detected as `Escalated`. A
    /// provider that always advances the current station and NEVER completes a
    /// wave seeds the station's wave once but never marks its unit done, so the
    /// engine's own deadlock guard fires `Escalate`. This inducer never returns
    /// `Stop`.
    #[test]
    fn escalate_scenario_is_detected_red() {
        struct AlwaysAdvance;
        impl Provider for AlwaysAdvance {
            fn next_move(&mut self, _prompt: Option<&str>) -> ProviderMove {
                ProviderMove::AdvanceStation
            }
        }
        let world = World::new("dark-escalate", "software");
        let result = world.drive(&mut AlwaysAdvance);
        assert!(
            matches!(result.outcome, WorldOutcome::Escalated { .. }),
            "the no-progress loop must be detected as Escalated, got {:?}",
            result.outcome
        );
    }

    /// Contract 1 (named): the move sequence resolves targets from state reads,
    /// but the reads never change WHICH move is chosen. Driving the same script
    /// twice — once clean, once corrupting every non-identifier state field after
    /// each tick — yields identical move sequences.
    #[test]
    fn state_reads_resolve_targets_only() {
        let clean = World::new("dark-targets-clean", "software");
        let clean_moves = clean.drive(&mut ScriptedProvider::new(dark_core_script())).moves;

        let dirty = World::new("dark-targets-dirty", "software");
        let dirty_moves = dirty
            .drive_corrupting(&mut ScriptedProvider::new(dark_core_script()))
            .moves;

        assert!(!clean_moves.is_empty(), "the drive emitted moves");
        assert_eq!(
            clean_moves, dirty_moves,
            "corrupting non-identifier state changed the move sequence — a decision leaked from state"
        );
    }

    /// The default scripted dark scenario reaches `Sealed`.
    #[test]
    fn dark_core_scenario_seals() {
        let world = World::new("dark-seals", "software");
        let result = world.drive(&mut ScriptedProvider::new(dark_core_script()));
        assert_eq!(result.outcome, WorldOutcome::Sealed);
        assert!(!result.prompts.is_empty());
    }

    /// Edge (post-`Sealed`): the loop stops at the first `Sealed`; a manual extra
    /// tick still returns `Sealed` (the engine's idempotence contract), which the
    /// world's normal operation never depends on.
    #[test]
    fn extra_tick_after_sealed_is_idempotent() {
        let world = World::new("dark-post-sealed", "software");
        let result = world.drive(&mut ScriptedProvider::new(dark_core_script()));
        assert_eq!(result.outcome, WorldOutcome::Sealed);
        let extra =
            run_tick_with_hosting(&world.store, &world.slug, &NoopHosting).expect("extra tick");
        assert_eq!(
            grade_tick(&extra),
            Some(WorldOutcome::Sealed),
            "a post-Sealed tick still returns Sealed"
        );
    }

    /// Edge (a rejected move): a mechanical move the engine rejects is a harness
    /// failure, not a protocol finding. `elaborate_seal` against a station that
    /// is not active returns `McpError` — and the world's `execute_advance_station`
    /// `.expect()`s that Result rather than swallowing it, so a rejected move
    /// surfaces as a loud panic, never a scored `FixtureOutcome`. This asserts the
    /// rejection is a real `Err` (the harness-failure class, per AC-10).
    #[test]
    fn a_rejected_move_is_a_harness_failure() {
        let world = World::new("dark-reject", "software");
        let rejected = elaborate_seal(&world.store, &world.slug, "not-a-real-station");
        assert!(
            rejected.is_err(),
            "the engine must reject a move against a non-active station — the world .expect()s this"
        );
    }

    /// Edge (an exhausted script): `Stop` returned before a terminal outcome is
    /// observed is a harness-failure panic naming the exhausted step count.
    #[test]
    #[should_panic(expected = "scripted sequence is exhausted")]
    fn an_exhausted_script_panics() {
        // Two moves then Stop — nowhere near sealing.
        let script = vec![
            ProviderMove::AdvanceStation,
            ProviderMove::AdvanceStation,
            ProviderMove::Stop,
        ];
        let world = World::new("dark-exhausted", "software");
        let _ = world.drive(&mut ScriptedProvider::new(script));
    }

    /// Edge (a double `run_start`): the world's own guard prevents a second start
    /// against a slug this store already carries.
    #[test]
    #[should_panic(expected = "double run_start guard")]
    fn double_run_start_is_prevented_by_the_world_guard() {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::new(dir.path());
        run_start(&store, "dup-run", "software", None, Mode::Dark, "full")
            .expect("first run_start");
        assert_fresh_start(&store, "dup-run");
    }

    /// Edge (a dark-mode `FeedbackQuestion`): `walk_feedback` runs every tick
    /// regardless of mode, so a seeded open question surfaces as
    /// `feedback_question` on the feedback track. Confirmed by reading the
    /// action-log journal, never the structured action.
    #[test]
    fn dark_mode_feedback_question_surfaces() {
        let world = World::new("dark-fq", "software");
        let station = world.active_station().expect("an active station");
        let doc = format!(
            "---\nstatus: pending\nstation: {station}\nkind: question\n---\nWhich option do you want?\n"
        );
        world
            .store
            .write_feedback_raw(&world.slug, "fb-q-01", &doc)
            .expect("seed a question");
        let tick =
            run_tick_with_hosting(&world.store, &world.slug, &NoopHosting).expect("tick");
        assert!(tick.prompt.is_some(), "a feedback question renders a prompt");
        let log = world.store.read_journal(&world.slug, "action-log.jsonl");
        let last = log.last().expect("an action-log entry");
        let entry: serde_json::Value = serde_json::from_str(last).expect("action-log json");
        assert_eq!(entry["action"], "feedback_question");
        assert_eq!(entry["track"], "feedback");
    }
}
