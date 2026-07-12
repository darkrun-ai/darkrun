---
station: build
phase: pre
created_at: 2026-07-12T05:48:45.800282+00:00
---
# Build spec — ready for review

**What this station does:** implements the locked spec (16 ACs, six contracts as amended by fb-08) across four dependency-chained Units:

1. **fixture-schema** (darkrun-core, sonnet) — the wasm-safe `SimFixture`/`FixtureTick`/`FixtureEvent`/`FixtureUnit`/`FixtureOutcome` serde types + `SIM_FIXTURE_SCHEMA_VERSION`, with round-trip tests. Gates: `cargo test -p darkrun-core sim_fixture`, crate clippy, no-new-deps.
2. **sim-spine** (crates/darkrun-sim, opus, depends fixture-schema) — the frame-compliant world/provider/transcript modules, the harness rebuild (run_tick_with_hosting + vendored NoopHosting, capture_to_seal relocated verbatim to scenarios.rs), the five named tests from the spec (prompt-blindness behavioral test, state-reads boundary test, regenerate-twice determinism, red-on-Escalate inducer, committed-fixture match), and the committed `fixtures/dark-core.json`. Gates: full crate tests (followability untouched), clippy, the seam greps (no bare run_tick, zero .action in world/provider, walk loop moved), fixture parse assertions.
3. **replay-route** (web/site, sonnet, depends both) — `/replay` page from the embedded fixture, all three darkrun-ui components (UnitGraph fed by the fb-08 units field), banner + no-fetch, route + all_paths registration. Gates: site tests, wasm32 clippy, the AC-13 greps.
4. **ci-gates** (.github/workflows, sonnet, depends 2+3) — the new dedicated `sim-fidelity` job (operator decision): regenerate-diff test, negative Escalate scenario, darkrun-site wasm clippy. Gates: YAML parse + step presence + local dry-run of the job's commands.

**Discovery folded in:** the reuse explorer's map (walk loop relocates verbatim; NoopHosting mirrors hosting.rs's private Stub; route/embedding/components are mechanical /preview copies; the two genuinely new pieces are the prompt-blind tick loop and the three-stream projector) and the integration explorer's seam map (exact import paths — elaborate_seal/run_tick_with_hosting/Hosting live at module paths, not crate root; followability.rs imports only core_scenarios; scenarios.rs's call site flips to free-function form; serde derive feature already enables Deserialize workspace-wide; the darkrun-site wasm CI blind spot is real and pre-existing).

**Operator decisions folded in (2026-07-11/12, this session):** 4-unit chain over finer split; new dedicated sim-fidelity CI job over extending wasm-app; the fb-08 spec amendment adding `units: Vec<FixtureUnit>` to the fixture schema so UnitGraph renders real nodes/edges (routed through the feedback track, applied to the run-main store copy).

**Out of scope for the station:** engine-code changes of any kind, real-model provider, web/app, solo/team gate simulation — all per the frame's and spec's exclusions.

**Next:** reviewer pass (correctness + maintainability) over the unit specs, then your pre-execution gate releases the first wave.
