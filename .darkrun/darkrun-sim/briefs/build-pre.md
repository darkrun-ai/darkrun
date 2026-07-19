---
station: build
phase: pre
created_at: 2026-07-12T06:13:00.242448+00:00
---
# Build spec — reviewed and ready for the pre-execution gate

**The decomposition:** four dependency-chained Units implementing the locked spec (16 ACs, six contracts as amended by fb-08): (1) fixture-schema — the wasm-safe SimFixture serde types in darkrun-core, snake_case wire form pinned to the domain.rs idiom; (2) sim-spine (opus) — the world/provider/transcript modules, harness rebuild onto run_tick_with_hosting + vendored NoopHosting, the walk loop relocated verbatim to scenarios.rs, five named tests, and the committed fixtures/dark-core.json; (3) replay-route — the /replay page from the embedded fixture with all three darkrun-ui components (UnitGraph fed by the fb-08 units field), banner, no-fetch; (4) ci-gates — the dedicated sim-fidelity CI job (regenerate-diff, red-on-Escalate, darkrun-site wasm32 clippy). Operator decisions folded in: 4-unit chain, dedicated CI job, fb-08 schema amendment.

**Review record — both lenses signed off after two fix rounds:**
- **correctness (strict) — filed fb-10 (high), then fb-12 (high) against the first fix, then stamped.** Findings, all in machine-executed gates: the fixture gate asserted "sealed" where bare serde emits "Sealed" (resolved by pinning #[serde(rename_all = "snake_case")] per the crate idiom, with a wire-form test); the .action seam grep was stricter than AC-3 and conflicted with Contract 1's own doc comment (resolved by strict-zero on provider.rs plus grading confinement to a column-0 fn grade_tick); the cargo-test name-filter gate passed vacuously on zero matches (resolved with a nonzero-passed output assertion); and the first confinement fix's shell quoting made the leg vacuous (resolved with the reviewer's tested [.]action/^fn-anchored drop-in plus a count >= 1 clause). Final verification: five probe cases against the stored gate verbatim — violation fails, compliant passes, zero-count fails, indented declaration fails closed, pre-work tree fails non-vacuously. Stamped all four units.
- **maintainability — filed fb-09 (medium), then fb-11 (medium) against the same quoting defect, then stamped.** Findings: the serde-casing ambiguity let two units independently guess and disagree (same resolution as above — single source of truth now stated in both bodies); the vacuous confinement leg (same fix, independently verified with its own violation mock, five scenarios all correct). Convention fit, blast radius, sprawl guards, and doc-comment durability all re-checked clean. Stamped all four units.

All four feedback items resolved as addressed with empirical evidence recorded on each. Review stamps on record: correctness + maintainability (kind: review) across fixture-schema, sim-spine, replay-route, ci-gates.

**Next:** your pre-execution gate. Approving releases the first wave (fixture-schema) through the build workers (test_author → builder → self_reviewer → reconciler), then sim-spine, replay-route, and ci-gates in dependency order.
