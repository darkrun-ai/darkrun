---
station: frame
phase: pre
created_at: 2026-07-02T23:52:43.007560+00:00
---
**What frame must kill:** building the wrong thing. For darkrun-sim, "wrong thing" has four concrete faces, all now grounded in explorer evidence:

1. **Build-quality drift.** The sim proves protocol fidelity — a red run means "a zero-privileged-knowledge agent got stranded or the engine emitted an unfollowable prompt," never "the produced software doesn't compile." The e2e suite (`crates/darkrun-e2e/tests/common/mod.rs::run_to_seal`) drives the real engine but reads only `TickResult.action` with privileged pokes (`elaborate_seal`, `run_review_stamp`, direct unit completion) and never reads `TickResult.prompt` — so today's green proves cursor termination, not followability. That gap is the product.
2. **Re-implementing engine mechanics in the harness.** The sim consumes the rendered prompt surface only: in-process library calls (`StateStore::new` → `run_start` → `run_tick_with_hosting` with a no-op Hosting), acting on `.prompt`, never on `.action`. The prompt corpus contains no machine-parseable dispatch markup (verified) — no dispatch-block parser gets built.
3. **Live engine per website visitor.** Recording happens locally at record time; the persisted transcript becomes a committed fixture; the website ships a static replay player. This amplifies the site's existing architecture (`/preview` renders without a running engine; `/browse` reads committed `.darkrun/` trees; every tick already persists rendered prompts + action-log + events under `.darkrun/<slug>/`).
4. **Coupling to engine internals.** The contract is `TickResult { action, position, prompt }` plus the persisted transcript files — never position-derivation internals. The engine's own `deadlock.rs` `Escalate` (HALT_THRESHOLD=4) is the machine-readable stranded verdict the sim keys red on.

**Placement:** new crate `crates/darkrun-sim` (consuming darkrun-mcp as a library, like darkrun-e2e does); replay player in `web/site` alongside `/preview`. **Build order:** world+transcript spine (scripted, no LLM) → replay player panels → real recording provider (dumb model) → site/CI fixture consumption.

**Going to the operator now:** (a) how simulated runs handle operator-collaboration gates — dark-mode spine first vs scripted operator-sim from day one (solo-mode Spec holds on `elaborate_seal`, so a pure prompt-follower needs an operator in the loop or a dark-mode run); (b) the recording provider class for the dumb agent. Decompose follows their answers: frame.md authoring unit(s) with referential-integrity quality gates (every cited repo path must exist).
