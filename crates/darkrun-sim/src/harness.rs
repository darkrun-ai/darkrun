//! Drives a **real** engine Run and captures the rendered prompt text for each
//! tick — the narrowed primitives the prompt-wording linter (`scenarios.rs` +
//! `tests/followability.rs`) is built on.
//!
//! This is the fidelity boundary for the linter partition: nothing here mocks
//! the engine. It owns a real [`StateStore`] in a `tempfile::TempDir` and offers
//! the mechanical building blocks — start a Run, tick it, seal a station's
//! elaboration, decide a checkpoint, render an action's prompt, decompose a
//! wave unit, complete units — that the walk-until-`Sealed` loop in
//! `scenarios.rs` composes. The loop itself lives in `scenarios.rs`, not here:
//! `harness.rs` makes no decision keyed on the structured action.
//!
//! The one tick entry point is [`run_tick_with_hosting`] against the sim's
//! [`crate::world::NoopHosting`] — never the network-reaching `run_tick`.

use darkrun_core::domain::{Mode, Status, Unit, UnitFrontmatter};
use darkrun_core::StateStore;
use darkrun_mcp::position::{
    checkpoint_decide, elaborate_seal, render_prompt, run_start, run_tick_with_hosting, RunAction,
    TickResult,
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

    /// Tick once, against the sim's no-op hosting client (never the
    /// network-reaching `run_tick`).
    pub fn tick(&self) -> TickResult {
        run_tick_with_hosting(&self.store, &self.slug, &crate::world::NoopHosting).expect("tick")
    }

    /// Clear a station's Spec-elaboration hold.
    pub fn seal(&self, station: &str) {
        elaborate_seal(&self.store, &self.slug, station).expect("seal");
    }

    /// Decide the active checkpoint.
    pub fn decide(&self, approved: bool, feedback: Option<&str>) -> TickResult {
        checkpoint_decide(&self.store, &self.slug, approved, feedback.map(String::from))
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
    pub(crate) fn decompose_one(&self, station: &str, unit_slug: &str) {
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
    pub(crate) fn complete_units(&self, slugs: &[&str]) {
        for s in slugs {
            let mut u = self.store.read_unit(&self.slug, s).expect("read_unit");
            u.frontmatter.status = Status::Completed;
            self.store.write_unit(&self.slug, &u).expect("write_unit");
        }
    }

    /// Ensure a station owes a wave: decompose one unit if it has none yet, then
    /// clear its Spec hold.
    pub(crate) fn seed_spec(&self, station: &str) {
        let unit = format!("{station}-unit");
        if self.store.read_unit(&self.slug, &unit).is_err() {
            self.decompose_one(station, &unit);
        }
        self.seal(station);
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
