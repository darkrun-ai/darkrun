//! The agent-side decision seam: the `Provider` trait, its `ProviderMove`
//! vocabulary, and the scripted, deterministic, no-model implementation this
//! Run ships (locked decision 2).
//!
//! The trait's single decision method takes `prompt: Option<&str>` — mirroring
//! `TickResult.prompt`'s type exactly — and NOTHING else. It carries no
//! parameter for the structured action variant the engine also returns: the
//! interface structurally withholds it, mirroring the frame's "read only
//! `.prompt`" seam. A real dumb-model recorder (a later phase) would read the
//! prompt and decide; the scripted provider here reads the prompt and ignores
//! it, returning a pre-determined move regardless of the wording. That is why a
//! scripted-green run validates the harness spine, not prompt followability.

/// One decision the agent-side driver can make this tick. The `world` module
/// resolves WHICH station or unit each targeted variant aims at using its own
/// per-tick `StateStore` reads — the move names the KIND of action, never the
/// target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderMove {
    /// Seed the current station's wave (mirrors the existing linter's
    /// `Harness::seed_spec`) and clear its elaboration hold. Idempotent: a
    /// station that already owes a wave only has its hold re-cleared.
    AdvanceStation,
    /// Mark every unit in the current wave completed.
    CompleteWave,
    /// Approve the current hold (a `UserGate`, a non-auto `Checkpoint`, or an
    /// `ExternalReviewRequested`) with no feedback.
    Approve,
    /// Stamp every named run-level reviewer.
    StampRunReviewers,
    /// Stop driving. Legitimate ONLY when the `world` module has already
    /// observed a terminal outcome (sealed or escalated) on the MOST RECENT
    /// tick — that is the one and only rule. A scripted provider with nothing
    /// left to do before a terminal outcome has a scenario bug, not a `Stop` to
    /// return; the `world` module treats a `Stop` seen with no terminal outcome
    /// observed as a harness failure and panics, naming the exhausted step
    /// count, rather than looping or silently no-oping.
    Stop,
}

/// The agent-side decision seam. One implementation ships this Run: the
/// scripted [`ScriptedProvider`] below.
pub trait Provider {
    /// One decision cycle. `prompt` is the CURRENT tick's `TickResult.prompt`
    /// (`None` for a tick with no rendered text). The signature carries no
    /// parameter for the structured action variant — the interface
    /// structurally withholds it, mirroring the frame's "read only `.prompt`"
    /// seam.
    fn next_move(&mut self, prompt: Option<&str>) -> ProviderMove;
}

/// The scripted, deterministic, no-model provider: a fixed move sequence played
/// back in order. It receives each tick's rendered prompt as its trait input —
/// the same input a real provider would get — but conditions its return value
/// ONLY on its own private cursor into the fixed sequence. It never parses,
/// matches on, or otherwise branches on the prompt's content. Once the sequence
/// is exhausted it returns [`ProviderMove::Stop`], so a driver that asks for
/// more moves than the script supplies fails loudly instead of looping.
pub struct ScriptedProvider {
    /// The fixed move sequence, played front to back.
    moves: Vec<ProviderMove>,
    /// The private cursor — the ONLY thing `next_move` conditions on.
    cursor: usize,
}

impl ScriptedProvider {
    /// Build a scripted provider from a fixed move sequence.
    pub fn new(moves: Vec<ProviderMove>) -> Self {
        Self { moves, cursor: 0 }
    }

    /// How many moves this provider has emitted so far — the "exhausted step
    /// count" the `world` module names when a premature `Stop` surfaces.
    pub fn emitted(&self) -> usize {
        self.cursor
    }
}

impl Provider for ScriptedProvider {
    fn next_move(&mut self, _prompt: Option<&str>) -> ProviderMove {
        // The prompt parameter is deliberately unread: the scripted provider is
        // prompt-blind. Only the private cursor selects the move.
        let mv = self
            .moves
            .get(self.cursor)
            .copied()
            .unwrap_or(ProviderMove::Stop);
        self.cursor += 1;
        mv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The prompt-blindness contract (AC-5): the same script, driven twice —
    /// once with real rendered prompts, once with every prompt replaced by a
    /// fixed dummy string — emits an IDENTICAL move sequence. If the scripted
    /// provider ever conditioned on the prompt, the two sequences would diverge.
    #[test]
    fn scripted_provider_ignores_prompt_content() {
        let script = vec![
            ProviderMove::AdvanceStation,
            ProviderMove::AdvanceStation,
            ProviderMove::CompleteWave,
            ProviderMove::AdvanceStation,
            ProviderMove::Approve,
            ProviderMove::StampRunReviewers,
            ProviderMove::Stop,
        ];
        // Real, varied prompt text — the kind the engine renders per tick.
        let real_prompts: [Option<&str>; 7] = [
            Some("Spec the station: run Explorers, then decompose into Units."),
            Some("Review the station's spec before any output is manufactured."),
            Some("Manufacture: run the Pass loop over the wave-ready Units."),
            Some("Audit the manufactured output against the spec."),
            Some("The station's checkpoint gate is open; surface it."),
            Some("Every station is locked — stamp the run reviewers."),
            None,
        ];

        let mut with_real = ScriptedProvider::new(script.clone());
        let mut with_dummy = ScriptedProvider::new(script.clone());

        let mut real_seq = Vec::new();
        let mut dummy_seq = Vec::new();
        for prompt in real_prompts {
            real_seq.push(with_real.next_move(prompt));
            // Every prompt replaced by the SAME fixed dummy string.
            dummy_seq.push(with_dummy.next_move(Some("<dummy prompt — content is irrelevant>")));
        }

        assert_eq!(
            real_seq, dummy_seq,
            "the scripted provider must emit the same moves regardless of prompt content"
        );
        assert_eq!(real_seq, script, "the emitted sequence is exactly the script");
    }

    /// A `None` prompt (a tick with no rendered text) is also ignored — the
    /// provider never dereferences the parameter.
    #[test]
    fn none_prompt_is_ignored_too() {
        let script = vec![ProviderMove::CompleteWave, ProviderMove::Stop];
        let mut a = ScriptedProvider::new(script.clone());
        let mut b = ScriptedProvider::new(script);
        assert_eq!(a.next_move(None), b.next_move(Some("anything")));
        assert_eq!(a.next_move(Some("x")), b.next_move(None));
    }

    /// Past the end of the script, the provider returns `Stop` indefinitely —
    /// the signal the `world` module turns into an exhausted-script panic when
    /// no terminal outcome has been observed.
    #[test]
    fn exhausted_script_returns_stop() {
        let mut p = ScriptedProvider::new(vec![ProviderMove::AdvanceStation]);
        assert_eq!(p.next_move(None), ProviderMove::AdvanceStation);
        assert_eq!(p.next_move(None), ProviderMove::Stop);
        assert_eq!(p.next_move(None), ProviderMove::Stop);
        assert_eq!(p.emitted(), 3);
    }
}
