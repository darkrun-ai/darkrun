//! Drives a **real** engine Run and captures the rendered prompt text for each
//! tick.
//!
//! This is the fidelity boundary: nothing here mocks the engine. It owns a real
//! [`StateStore`] in a `tempfile::TempDir` and walks it through the same
//! `run_start → run_tick → checkpoint_decide` path the production MCP server
//! uses, then hands the [`TickResult::prompt`] — the exact instruction text an
//! agent would read — to the [`SimAgent`](crate::agent::SimAgent). A
//! prompt-wording regression therefore surfaces as a failing followability
//! assertion, not a silent unfollowable instruction.
//!
//! The walk logic mirrors the darkrun-e2e driver: it reads the manager's next
//! action and reacts to it (decompose a unit when a station owes a spec, complete
//! the wave, approve a held gate), looping until the Run seals.

use std::collections::BTreeMap;

use darkrun_core::domain::{CheckpointKind, Mode, Status, Unit, UnitFrontmatter};
use darkrun_core::StateStore;
use darkrun_mcp::position::{
    checkpoint_decide, elaborate_seal, render_prompt, run_review_stamp, run_start, run_tick,
    RunAction, TickResult,
};

/// A self-contained Run fixture: a temp dir + the store rooted in it + the run
/// slug. Dropping it tears down all on-disk state.
pub struct Harness {
    _dir: tempfile::TempDir,
    /// The real on-disk state store backing the Run.
    pub store: StateStore,
    /// The Run slug.
    pub slug: String,
}

impl Harness {
    /// Start a fresh Run of `factory` in `mode` (`"solo"` / `"team"` / `"dark"`),
    /// pre-sealing the first station's elaboration so its Spec prompt renders in
    /// its steady-state (post-collaboration) form.
    pub fn start(slug: &str, factory: &str, mode: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = StateStore::new(dir.path());
        run_start(&store, slug, factory, None, Mode::from_label(mode), "full").expect("run_start");
        let h = Harness {
            _dir: dir,
            store,
            slug: slug.to_string(),
        };
        // Pre-seal the opening station: under team/solo every station HOLDS its
        // Spec until elaborated, which lights the collaboration back-pressure
        // block. Sealing the first station clears that hold so its Spec prompt
        // renders the steady-state instruction the sim reads.
        if let Some(first) = h.active_station() {
            h.seal(&first);
        }
        h
    }

    /// The active-station pointer, if state exists.
    fn active_station(&self) -> Option<String> {
        self.store
            .read_state(&self.slug)
            .ok()
            .flatten()
            .map(|s| s.active_station)
    }

    /// Tick once.
    pub fn tick(&self) -> TickResult {
        run_tick(&self.store, &self.slug).expect("tick")
    }

    /// Clear a station's Spec-elaboration hold.
    pub fn seal(&self, station: &str) {
        elaborate_seal(&self.store, &self.slug, station).expect("seal");
    }

    /// Decide the active checkpoint.
    pub fn decide(&self, approved: bool, feedback: Option<&str>) -> TickResult {
        checkpoint_decide(
            &self.store,
            &self.slug,
            approved,
            feedback.map(String::from),
        )
        .expect("decide")
    }

    /// Render an arbitrary action's prompt against this Run's real state — the
    /// same [`render_prompt`] the tick uses, for actions a linear walk does not
    /// naturally reach (e.g. a feedback question).
    pub fn render(&self, action: &RunAction) -> Option<String> {
        render_prompt(&self.store, &self.slug, action).expect("render_prompt")
    }

    /// Decompose one wave unit on `station`, consuming the station's declared
    /// inputs so the runtime input-coverage gate is satisfied.
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
        self.store
            .write_unit(&self.slug, &unit)
            .expect("write_unit");
    }

    /// Mark every named unit completed.
    fn complete_units(&self, slugs: &[&str]) {
        for s in slugs {
            let mut u = self.store.read_unit(&self.slug, s).expect("read_unit");
            u.frontmatter.status = Status::Completed;
            self.store.write_unit(&self.slug, &u).expect("write_unit");
        }
    }

    /// Ensure a station owes a wave: decompose one unit if it has none yet, then
    /// clear its Spec hold.
    fn seed_spec(&self, station: &str) {
        let unit = format!("{station}-unit");
        if self.store.read_unit(&self.slug, &unit).is_err() {
            self.decompose_one(station, &unit);
        }
        self.seal(station);
    }

    /// Walk the Run to a sealed state, capturing the **first** rendered prompt
    /// seen for each distinct action tag (`spec`, `review`, `manufacture`,
    /// `audit`, `reflect`, `user_gate`, `checkpoint`, …). The map is what the
    /// followability suite reads.
    pub fn capture_to_seal(&self) -> BTreeMap<String, String> {
        let mut prompts: BTreeMap<String, String> = BTreeMap::new();
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 2000, "capture_to_seal failed to converge");
            let tick = self.tick();
            let action = tick.action.clone();
            if let Some(p) = &tick.prompt {
                prompts
                    .entry(action_tag(&action))
                    .or_insert_with(|| p.clone());
            }

            // The pre-execution operator gate is an internal hold — approve and
            // keep walking.
            if matches!(action, RunAction::UserGate { .. }) {
                self.decide(true, None);
                continue;
            }
            // The whole-run review holds until every reviewer signs — stamp them.
            if let RunAction::RunReview { reviewers, .. } = &action {
                for r in reviewers {
                    run_review_stamp(&self.store, &self.slug, r).expect("run review stamp");
                }
                continue;
            }

            match &action {
                RunAction::Sealed { .. } => break,
                RunAction::Spec { station, .. } => self.seed_spec(station),
                RunAction::Manufacture { units, .. } => {
                    let owned: Vec<&str> = units.iter().map(|s| s.as_str()).collect();
                    self.complete_units(&owned);
                }
                // A held gate (non-auto checkpoint, or an external review gate)
                // needs an operator decision; the decide re-tick advances the
                // next station, so re-seed its spec to stay in sync.
                RunAction::Checkpoint { kind, .. } if !matches!(kind, CheckpointKind::Auto) => {
                    let decided = self.decide(true, None);
                    if let RunAction::Spec { station, .. } = &decided.action {
                        self.seed_spec(station);
                    }
                }
                RunAction::ExternalReviewRequested { .. } => {
                    let decided = self.decide(true, None);
                    if let RunAction::Spec { station, .. } = &decided.action {
                        self.seed_spec(station);
                    }
                }
                _ => {}
            }
        }
        prompts
    }
}

/// The `action` discriminator serde emits for a [`RunAction`] (`spec`,
/// `checkpoint`, `feedback_question`, …).
pub fn action_tag(action: &RunAction) -> String {
    serde_json::to_value(action)
        .ok()
        .and_then(|v| v["action"].as_str().map(String::from))
        .unwrap_or_default()
}
