---
station: frame
phase: pre
created_at: 2026-07-03T01:25:50.034918+00:00
---
# Frame spec — reviewed and ready for the pre-execution gate

**What this station does:** locks the scope frame for darkrun-sim, a protocol-fidelity simulator. A zero-privileged-knowledge agent drives the real engine through its rendered prompts alone; red = agent stranded (engine `Escalate` from `crates/darkrun-mcp/src/deadlock.rs`) or prompt unfollowable — never "the produced software doesn't compile." One Unit (`author-frame`, opus, document-class) writes the locked artifact `frame/frame.md` carrying: the verified e2e gap (the privileged driver reads `TickResult.action`, never `.prompt`), the four bounded wrong-thing exclusions, the sole coupling seam (`TickResult` + persisted transcript files, driven via `run_tick_with_hosting` with a no-op Hosting), your two locked decisions with re-entry triggers, and the dependency-sequenced build order (spine → replay player in web/site → site/CI consumption).

**Operator decisions folded in (q-01/q-02, 2026-07-02):** dark-mode spine first with the scripted operator-sim deferred (trigger: spine + player merged and green in CI); scripted provider only with the real-model recorder deferred (trigger: replay page live with a committed scripted fixture).

**Review record — both lenses signed off, zero feedback filed:**
- **value — stamped.** Red-run definition unambiguous and triple-barred against build-quality misread; all four wrong-thing boundaries carry checkable conditions; both deferrals carry named re-entry triggers; the Unit's `citations-resolve` gate (≥8 distinct extension-bearing repo paths, all resolving on disk) kills the cite-vaporware failure mode. Two candidate objections were considered and correctly NOT filed — each would have relitigated the locked q-02 decision.
- **feasibility — stamped.** Every seam claim verified true in-repo (`run_to_seal` has zero `prompt` reads; `run_tick_with_hosting` pub at `position.rs:2386`; `Escalate` swap + 3600s reset in `deadlock.rs`; `write_prompt`/`read_prompts` in `darkrun-core`; no `<subagent` markup anywhere in `plugin/prompts/`; `adapt_instructions` literally returns input unchanged for the Claude Code cap set). Gates dry-run on macOS/BSD tooling: citation-free output fails the ≥8 floor, a planted bogus path fails `xargs -I{} test -e {}`, the real set passes. `.darkrun/darkrun-sim/` is committed on the run branch, so the Unit's isolated worktree resolves both the artifact path and the cited source tree. Document-class scope honored per `enforce_unit_scope`. Phase 1 buildable now: the `Hosting` trait and `run_tick_with_hosting` are pub, no LLM and no git remote required.
- **One nuance recorded, not a defect:** e2e's generic harness calls plain `run_tick`; the spec's `run_tick_with_hosting` + no-op Hosting directive is the deliberately safer variant for the sim (network-free by construction).

**Next:** your pre-execution gate. Approving dispatches the `author-frame` Unit through the frame workers (framer → challenger → distiller).
