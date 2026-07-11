
> **Run** `darkrun-sim` · **Station** `specify` · **Phase** `manufacture`

> Eliminates: _ambiguity_


# Manufacture — `specify`

This is the build floor. You run the **Pass loop** — _Plan → Make → Challenge → Resolve_ — over the wave-ready Units. The current beat is **spec_writer**, on model **sonnet**.


**Contract**

- Do exactly the work this action describes — no more, no less. Don't skip ahead to a later phase.
- Treat the locked artifact (`spec.md`) as the source of truth. Read it before you act; never silently rewrite a locked decision.
- Every claim you make must be backed by something you actually ran, read, or wrote. No assumed results.
- Be specific and committed. **No placeholders** — a `TBD`, `similar to …`, `add error handling`, `etc.`, or `…` is a hole, not a decision; name the actual, checkable condition. **No hedging** — when you report work done, use a verb of completed action (`added`, `implemented`, `fixed`), never `should`, `seems`, `probably`, `might`, or `looks like`. Hedging is the tell of unfinished work.
- When the action is finished, record your output where the station expects it, then call `darkrun_tick` again for the next instruction. The manager — not you — decides what comes next.



**Explorers** (2): `contract`, `edge_case`


**Workers** (3): `spec_writer` → `adversary` → `tightener`


**Reviewers** (2): `testability`, `completeness`


## This wave


Dispatch the **spec_writer** beat in parallel across these wave-ready Units:

- `author-spec`




## Each Unit's spec — the contract the beat works against

The subagent you dispatch for a Unit gets **no context beyond what you hand it**. Pass the Unit's spec below into its dispatch verbatim — the completion criteria with their verify commands, the declared paths, and the scope boundary are the contract the beat is judged against.

### `author-spec` — Author spec.md — testable acceptance criteria, contracts, and edge cases for darkrun-sim

- **inputs:** `frame.md`


- **outputs:** `specify/spec.md`


- **quality gates:** artifact-nonempty — `test -s .darkrun/darkrun-sim/specify/spec.md` · citations-resolve — `sh -c 'n=$(grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/specify/spec.md | sort -u | wc -l); test "$n" -ge 10 && grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/specify/spec.md | sort -u | xargs -I{} test -e {}'` · acceptance-criteria-floor — `sh -c 'test $(grep -cE "^### AC-[0-9]+" .darkrun/darkrun-sim/specify/spec.md) -ge 12'`


# Unit: author-spec

## Goal

Write `spec.md` — the specify station's locked artifact for Run `darkrun-sim` — at `.darkrun/darkrun-sim/specify/spec.md` (relative to the worktree root you execute in). It turns the locked frame (`.darkrun/darkrun-sim/frame/frame.md` — read it first; never contradict it) into an unambiguous contract: testable acceptance criteria, boundary contracts, and edge-case behavior definitions. This document becomes Prove's rubric — anything vague here is unprovable later. Every criterion must have a yes/no answer an independent party could check without asking the author what they meant.

## Operator decisions (locked 2026-07-11; fold in verbatim, do not re-litigate)

1. **Partition the existing crate.** `crates/darkrun-sim` today is a prompt-wording linter (SimAgent text classifier in `crates/darkrun-sim/src/agent.rs`, tool-name registry in `crates/darkrun-sim/src/tool_registry.rs`, corpus tests in `crates/darkrun-sim/tests/followability.rs`) whose harness violates the frame's seams (plain `run_tick` at `crates/darkrun-sim/src/harness.rs` lines 21 and 69; `.action`-driven walk at lines 150-196; solo mode at `crates/darkrun-sim/src/scenarios.rs` line 53). The spec must partition: the linter modules stay as a distinct, still-valuable fidelity check; the frame-compliant simulator arrives as new modules (world, provider, transcript) in the same crate; `harness.rs` is rebuilt onto the locked seams (`run_tick_with_hosting`, prompt-only, `Mode::Dark`). Extending the current harness in place is forbidden.
2. **Fixture schema lives in darkrun-core.** `web/site` can never depend on `darkrun-mcp` (unconditional nix/tokio/ureq/rmcp deps; not wasm-clean), and `TickResult`/`RunAction`/`Position` are Serialize-only. The replay fixture type is a new wasm-safe serde struct set in `crates/darkrun-core` (its only native dep is cfg(unix)-gated), serialized by the sim and deserialized by the site.
3. **Determinism by normalization at projection.** The transcript projector strips/canonicalizes volatile fields when emitting the fixture; CI diffs projected fixtures, never raw prompt bytes. Known volatile sources (all verified): the `verifier_nonce` minted from `Utc::now()` (`crates/darkrun-mcp/src/position.rs` mint_verifier_nonce) and printed literally by `plugin/prompts/phases/manufacture.md`; every `at:` rfc3339 timestamp in `action-log.jsonl`/`events.jsonl`; iteration timestamps in unit frontmatter.

Plus the frame's four locked decisions (dark-mode spine first; scripted provider only behind a pluggable trait; placement `crates/darkrun-sim` + `web/site`; in-browser LLM dead) with their re-entry triggers — restate, never weaken.

## Verified facts to specify against (from the station's explorers; cite, do not re-derive)

- Consumption surface: `StateStore::new(repo_root)` (`crates/darkrun-core/src/state.rs`), `run_start(store, slug, factory, title, mode, size)` and `run_tick_with_hosting<H: Hosting>` (`crates/darkrun-mcp/src/position.rs`), `TickResult { run, position, action, prompt }`. `Hosting` needs exactly three non-defaulted methods for a no-op impl: `available()`, `open_draft()`, `merge_state()` (`crates/darkrun-mcp/src/hosting.rs`); darkrun-mcp exports no public stub, so the sim vendors its own `NoopHosting`.
- Transcript substrate: `StateStore::write_prompt` persists the CURRENT prompt per station/tag (overwritten, not append-only) to `.darkrun/<slug>/prompts/<scope>/<tag>.md`; `action-log.jsonl` lines are `{at, track, action, station}`; `events.jsonl` lines are `{at, event, run, ...fields}`; the two streams are NOT 1:1 (`darkrun.run.created` and `darkrun.station.dropped` have no action-log counterpart) — the spec must state the projector's explicit merge/ordering rule and that the fixture carries its own `schema_version`.
- Engine edges needing DEFINED behavior (each verified in-repo): a scripted move the engine rejects (`elaborate_seal` → InvalidInput when station not active; `checkpoint_decide` → NoActiveStation); a script exhausted before `Sealed`; `RunAction::FeedbackQuestion` firing in dark mode (mode-independent); `render_prompt` returning None for an unmapped action tag (never fires today — assert corpus-wide); the deadlock guard's `STALE_AGE_SECS=3600` history reset (`crates/darkrun-mcp/src/deadlock.rs`) silently zeroing the no-progress counter on wall-clock-slow runs; `save_wip` and `enforce_unit_scope` both no-op on the bare TempDir world (verified); repeated post-`Sealed` ticks return `Sealed` forever without escalating.
- Red verdict: `RunAction::Escalate` (deadlock guard: 4 same-signature no-progress ticks, or A-B churn over >=8 of 10, exemptions per `is_exempt`). The spec must ALSO classify the non-Escalate failure shapes above as red-verdict material with named transcript markers, or explicitly assign them to harness-failure (panic) status — no shape may be left undefined.
- Replay surface: new Route variant in `web/site/src/route.rs` AND an entry in its `all_paths()` static-render list (a route absent from `all_paths()` compiles and renders in dev while never reaching the static wasm export), modeled on `/preview` (`web/site/src/pages/preview.rs`: hardcoded payload, no fetch, explicit no-live-feed banner), composed from darkrun-ui prelude components (`StationStrip`, `StationPipeline` in `crates/darkrun-ui/src/components/pipeline.rs`, `UnitGraph` in `crates/darkrun-ui/src/graph/view.rs`). Fixture embedding precedent: `include_str!` (`web/site/src/content.rs`). CI gap: no workflow builds web/site for wasm32 today (`.github/workflows/ci.yml` wasm job scopes `-p darkrun-app` only) — the spec must state the Phase 3 CI gate shape (new job or extended job, regenerate-project-diff, plus the inverted red-on-Escalate negative scenario).

## Required document structure (in this order)

1. `# Spec: darkrun-sim — protocol-fidelity simulator` — one-paragraph contract overview naming what this spec makes checkable.
2. `## Acceptance criteria` — numbered `### AC-1` … `### AC-n` (n >= 12), each: one yes/no claim, the literal command or observable that checks it, and which build phase (1/2/3 from the frame) it lands in. Cover at minimum: world construction (bare TempDir, dark mode, no network), provider trait shape + scripted impl's prompt-blindness, prompt-only seam (zero `.action` reads in the driving path — checkable via grep over the new modules), transcript projection (three streams, merge rule, schema_version, normalization), fixture determinism (regenerate-twice byte-equal), red-on-Escalate detection, defined behavior for every non-Escalate failure shape, linter partition intact (existing followability tests still pass), replay route registered in BOTH the `web/site` Route enum and its `all_paths()` static-render list (checkable via grep on `web/site/src/route.rs`), replay page composed from the darkrun-ui prelude components (`StationStrip`, `StationPipeline`, `UnitGraph`) with an explicit no-live-feed banner and zero network fetch in the new page (checkable via grep for `gloo` / `remote::` / `fetch` over the new page module), replay route renders the committed fixture, CI gates (regenerate-diff green job + negative Escalate scenario), wasm boundary honored (web/site gains no darkrun-mcp edge — checkable via `cargo tree`).
3. `## Contracts` — six contracts: (1) the provider trait (name, method signatures, what the scripted impl may and may not condition on); (2) `NoopHosting` (the three methods and their return values); (3) the darkrun-core fixture schema (struct names, fields, `schema_version` field, normalization rules enumerated); (4) the crate module map after partition (which files exist, which are rebuilt, which are untouched); (5) the fixture file's committed path and embedding mechanism; (6) the replay Route contract (variant name, URL pattern, its `all_paths()` entry, the darkrun-ui components it composes, the no-live-feed banner, and the no-fetch rule).
4. `## Edge cases` — every edge listed above with its REQUIRED behavior (not options): rejected move, exhausted script, dark-mode FeedbackQuestion, render_prompt None, STALE_AGE reset, double run_start, post-Sealed ticks, empty prompts dir for unreached phases, fixture referencing content the site build no longer embeds.
5. `## Out of scope` — inherit the frame's exclusions; add: no engine-code changes for determinism (decision 3), no second sim crate (decision 1), no web/app work, no real-model provider.
6. `## Evidence` — path list of every file this spec's claims stand on.

## Completion criteria (verify each from the worktree root before finishing)

1. Artifact exists and is substantive → `test -s .darkrun/darkrun-sim/specify/spec.md` exits 0.
2. Exactly the six required H1/H2 headings, in the required order, with no extra H1/H2 headings → `sh -c 't=$(mktemp); grep -E "^##? " .darkrun/darkrun-sim/specify/spec.md > "$t"; printf "%s\n" "# Spec: darkrun-sim — protocol-fidelity simulator" "## Acceptance criteria" "## Contracts" "## Edge cases" "## Out of scope" "## Evidence" | diff - "$t"; r=$?; rm -f "$t"; exit $r'` exits 0 (AC subheadings are H3 `###` and are deliberately excluded by the `^##? ` pattern).
3. At least 12 acceptance criteria → `grep -cE '^### AC-[0-9]+' .darkrun/darkrun-sim/specify/spec.md` reports >= 12.
4. At least 10 distinct extension-bearing repo paths cited AND every one resolves → `sh -c 'n=$(grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/specify/spec.md | sort -u | wc -l); test "$n" -ge 10 && grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/specify/spec.md | sort -u | xargs -I{} test -e {}'` exits 0. Name planned-but-nonexistent artifacts (new modules, the fixture file, the new route) as directories or prose, never as extension-bearing paths.
5. Predecessor brand vocabulary absent → `grep -ci haiku .darkrun/darkrun-sim/specify/spec.md` reports 0.
6. No hedging or placeholder tokens; the banned token set is exactly {TBD, should, probably, might, seems, etc} → `grep -niE '\bTBD\b|\bshould\b|\bprobably\b|\bmight\b|\bseems\b|\betc\b' .darkrun/darkrun-sim/specify/spec.md` outputs nothing (exits 1). No time estimates → `grep -niE '\bweek(s)?\b|\bday(s)?\b|\bhour(s)?\b|\bmonth(s)?\b|timeline|deadline' .darkrun/darkrun-sim/specify/spec.md` outputs nothing (exits 1). Sequence only.

## Files touched

`.darkrun/darkrun-sim/specify/spec.md` only. Document-class station: touch no repo source files.

## Out of scope for this unit

No crate scaffolding, no site code, no CI changes, no edits to any other `.darkrun/` file, no re-opening of the frame's or today's operator decisions.




## Each Unit has its own worktree — work in it

Every wave Unit is isolated on its own branch + worktree, forked off the station branch. Run that Unit's beat **inside its worktree** so its diff never tangles with another Unit's in-flight work; the manager lands each Unit back onto the station branch when it locks. Do **not** commit a Unit's work to the station branch yourself.

- `author-spec` → `/Users/jwaldrip/dev/src/github.com/jwaldrip/darkrun/.claude/worktrees/wiggly-gathering-spark/.darkrun/worktrees/darkrun-sim/units/specify/author-spec` (branch `darkrun/darkrun-sim/units/specify/author-spec`)





## The Pass loop — make → challenge → resolve

The Pass loop is adversarial on purpose: a single confident pass is exactly where LLM output is most often confidently wrong, so a second pass red-teams the first before anything locks.

- **make** — the worker produces the Unit's output against its completion criteria. Build the real thing, not a sketch.
- **challenge** — a second pass attacks what make produced: edge cases, missing handling, lazy assumptions. Assume the first pass was optimistic.
- **resolve** — reconcile make and challenge into a Unit that satisfies its completion criteria with the challenges answered.




**Quality-gate verifier nonce.** This dispatch carries a one-time verifier token: **`2796f4eaea4e90bd84afe4b66881e47e69054c607d573f4730c6611de09041a1`**. When you record a quality gate with `darkrun_quality_gate_record`, pass it as `nonce`. The engine refuses a gate result without the matching token — so a gate is only ever recorded as part of a real verification dispatch, never self-certified. Run the gate's command for real, then record the actual outcome with this nonce.


Run **only the `spec_writer` beat** this tick. When the beat finishes, **record it** with `darkrun_unit_iterate` — pass the `worker`, the `result` (`advance` or `reject`), and a `note`: on advance, what you did and what the next worker needs to know; on reject, why you bounced it (a reject without a reason is refused). That note becomes the next beat's handoff above. Then call `darkrun_tick`; the manager advances the loop or releases the next wave. A Unit is locked only after Resolve and its completion criteria pass.

A Unit gets a **bounded pass budget** — the manager escalates a Unit that can't converge within it to the operator rather than grinding forever. Don't paper over a stuck Unit to dodge the escalation; a Unit that needs more passes than the budget allows is a signal the spec, the scope, or the approach is wrong, and that's the operator's call to make.



## Done when

The `spec_writer` beat is complete for every Unit in this wave and its output is recorded. Then call `darkrun_tick`.

---

# Provider contracts in effect

The project configures external-system providers whose behavior contracts apply to this phase. Follow them alongside the instructions above.

# Git Provider — Behavior Contract

darkrun is always git-backed when a `.git/` directory is present. This contract is **always active** in git environments — no settings activation needed.

## What you, the agent, must do

- Never run `git checkout`, `git merge`, `git branch -d`, or create branches manually during run operations. The engine owns branch topology, merge semantics, worktree creation, and station-branch enforcement.
- Commit substantive work (unit body edits, artifact writes, source changes) before calling `darkrun_tick` — the pre-tick clean-tree gate blocks the tick on loose agent work and hands the file list back. The engine commits its own `.darkrun/` state on every tick; it does NOT author your commits.
- **Never pair a VCS issue-closing keyword with a feedback id.** GitHub and GitLab parse `Closes`/`Fixes`/`Resolves`/`Implements` followed by an issue-shaped token as an external-issue closing reference — `Fixes fb-07` in a commit message or PR description renders a phantom closing link for a finding that is not a ticket. Use neutral phrasing — `addresses fb-07`, `per fb-07` — never a closing verb.
- Treat `git push` failures as non-fatal — the engine retries on the next tick. Don't block on a transient remote outage.
- If a station's gate is `external`, the engine watches for the PR merge signal. Don't flip frontmatter to fake the signal — the human's merge IS the decision.

## Branch architecture (read-only fact you operate against)

- **Run branch** `darkrun/<slug>/main` is the durable record. The engine commits state changes here and pushes on every tick (commit early, push often). The run's **delivery draft PR** opens against the project's default branch at run start and the engine flips it ready-for-review at seal.
- **Station branches** `darkrun/<slug>/<station>` accumulate station-scope work, synced downstream and landed by the engine.
- **Unit worktree branches** `darkrun/<slug>/units/<station>/<unit>` isolate each unit's diff — local-only, landed back onto the station branch when the unit locks.

## external_refs handling

The delivery PR's URL is stamped on `run.md` as `external_refs.pr_url` with its draft/ready status in `external_refs.other.pr_status`. You don't write these fields manually — the engine does — but you can read them to surface PR state to the operator. In DISCRETE mode the engine also opens a per-station draft PR at the station's external gate (recorded on `Station.pr_ref`); merging it is the approval.

## Proof asset uploads

Runtime-verification proof (screenshots, transcripts) is regenerated every run — attach it durably with `darkrun_proof_attach`, which records it on the run's proof ledger and posts it to the station's change request when one exists. Keep uploads idempotent — replace a re-run's proof rather than stacking duplicates.

## Non-git environments

When `.git/` is absent the engine falls back to filesystem persistence: no commits, no pushes, no worktrees, and `external` gates degrade to `ask` (there's no structural merge signal to enforce them). All run operations still work; this contract simply doesn't apply.