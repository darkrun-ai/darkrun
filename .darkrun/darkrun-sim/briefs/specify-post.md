---
station: specify
phase: post
created_at: 2026-07-11T18:47:15.855953+00:00
---
# Specify — what the station produced

**The locked artifact:** `specify/spec.md` on `darkrun/darkrun-sim/specify` (tip 58319e5, pushed) — 16 phase-tagged acceptance criteria, six contracts, nine edge cases with required outcomes, all six unit completion criteria passing (exact heading order, 29 resolving citations, zero brand/hedge/time tokens).

**Pass loop:** spec_writer authored (278c09a); adversary landed 3 blocking + 4 must-fix findings (AC-10/edge-case contradiction on STALE_AGE_SECS, three incompatible ProviderMove::Stop rules, the harness.rs/AC-11 walk-loop ownership hole, AC-3's filename-scoped grep leak, the overstated StateStore-reads precedent, the misattributed banner wording, the missing darkrun-site wasm CI gate); tightener resolved all seven (217840a). All three quality gates recorded pass twice (post-make and post-resolve).

**Audit:** testability and completeness each ran the checks for real, filed one genuine finding each instead of rubber-stamping — fb-06 (high): AC-5's prompt-blindness check was defeatable by renaming the parameter to `_prompt` (verified with rustc: underscore only silences the lint, reads still compile); fb-07 (medium): Contract 3 attributed the normalize-at-projection decision to the frame when it is a station-level operator decision. Both fixed in one 2-line commit (58319e5): AC-5 now requires a behavioral test (`scripted_provider_ignores_prompt_content` — same script, real vs dummy prompts, identical move sequences) plus a name-agnostic grep; Contract 3's attribution corrected. Both reviewers re-audited, attacked the fixes (four constructed counterexample bodies against the new grep — all caught), and stamped approval. Full check suite re-run green on the landed branch copy.

**What this spec pins down for Shape/Build:** the Provider trait and its prompt-blind scripted impl, the vendored NoopHosting, the darkrun-core SimFixture schema with enumerated normalization rules, the crate partition map (linter intact, walk loop to scenarios.rs, harness rebuilt on run_tick_with_hosting), the /replay Route contract (enum + all_paths(), darkrun-ui components, no-fetch, banner), and the CI gates (regenerate-diff on the normalized projection, red-on-Escalate negative scenario, darkrun-site wasm32 clippy).

**Next:** your checkpoint. Approving locks spec.md as Prove's rubric and advances the run.
