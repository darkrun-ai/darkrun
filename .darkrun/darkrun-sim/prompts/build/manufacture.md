
> **Run** `darkrun-sim` · **Station** `build` · **Phase** `manufacture`

> Eliminates: _implementation-defects_


# Manufacture — `build`

This is the build floor. You run the **Pass loop** — _Plan → Make → Challenge → Resolve_ — over the wave-ready Units. The current beat is **test_author**, on model **sonnet**.


**Contract**

- Do exactly the work this action describes — no more, no less. Don't skip ahead to a later phase.
- Treat the locked artifact (`code`) as the source of truth. Read it before you act; never silently rewrite a locked decision.
- Every claim you make must be backed by something you actually ran, read, or wrote. No assumed results.
- Be specific and committed. **No placeholders** — a `TBD`, `similar to …`, `add error handling`, `etc.`, or `…` is a hole, not a decision; name the actual, checkable condition. **No hedging** — when you report work done, use a verb of completed action (`added`, `implemented`, `fixed`), never `should`, `seems`, `probably`, `might`, or `looks like`. Hedging is the tell of unfinished work.
- When the action is finished, record your output where the station expects it, then call `darkrun_tick` again for the next instruction. The manager — not you — decides what comes next.



**Explorers** (2): `reuse`, `integration_point`


**Workers** (4): `test_author` → `builder` → `self_reviewer` → `reconciler`


**Reviewers** (2): `correctness`, `maintainability`


## This wave


Dispatch the **test_author** beat in parallel across these wave-ready Units:

- `replay-route`




## Each Unit's spec — the contract the beat works against

The subagent you dispatch for a Unit gets **no context beyond what you hand it**. Pass the Unit's spec below into its dispatch verbatim — the completion criteria with their verify commands, the declared paths, and the scope boundary are the contract the beat is judged against.

### `replay-route` — /replay route in web/site — static fixture player from darkrun-ui components

- **inputs:** `crates/darkrun-core/src/sim_fixture.rs`, `crates/darkrun-sim/fixtures/dark-core.json`


- **outputs:** `web/site/src/pages/replay.rs`


- **quality gates:** site-tests — `cargo test -p darkrun-site` · site-wasm-clippy — `cargo clippy -p darkrun-site --target wasm32-unknown-unknown -- -D warnings` · route-greps — `sh -c 'grep -q "/replay" web/site/src/route.rs && grep -qE "StationStrip|StationPipeline|UnitGraph" web/site/src/pages/replay.rs && grep -qiE "no.live.feed|no live feed" web/site/src/pages/replay.rs && ! grep -nE "gloo|remote::|\.fetch\(" web/site/src/pages/replay.rs'`


# Unit: replay-route

## Goal

Add the `/replay` route to `web/site` — a static, no-live-feed player that renders the committed fixture `crates/darkrun-sim/fixtures/dark-core.json` using darkrun-ui components. THE CONTRACT IS THE LOCKED SPEC: read `.darkrun/darkrun-sim/specify/spec.md` in full first — ACs 12-14 and Contract 6 are yours, including the fb-08 amendment (UnitGraph's nodes and edges come from the fixture's `units` field: each `FixtureUnit.slug` is a node, each `depends_on` entry an edge). Model the page on `/preview` (`web/site/src/pages/preview.rs`: hardcoded payload, zero fetch, explicit banner), NEVER on `/browse` (its `remote::fetch_*` live pattern is the banned shape).

## What to build

1. **`web/site/src/pages/replay.rs` (new).** A `#[component] pub fn Replay() -> Element` that: parses the fixture ONCE via `include_str!("../../../../crates/darkrun-sim/fixtures/dark-core.json")` + `serde_json::from_str::<darkrun_core::sim_fixture::SimFixture>` (path verified: four `..` from `web/site/src/pages/`); renders the no-live-feed banner whose wording follows `preview.rs` lines 88-93's `SectionHead` `lead` precedent ("…no live feed is attached." — the `ScaffoldNote` dashed-border container from `web/site/src/pages/review.rs` line 199 is an optional separate styling choice); composes all three prelude components against fixture-derived data: `StationStrip { stations: Vec<StationItem> }` from the distinct stations across `ticks` (status Done for stations before the last, Current for the last — derive from tick order), `StationPipeline { dots }` via `strip_for` from the final tick's phase-bearing action_tag mapped into `darkrun_ui::kinds::Phase` (the translation lives HERE; darkrun-ui is darkrun-core-free by design), and `UnitGraph { units, edges }` from the fixture's `units` field (`UnitGraphNode::new(slug, slug)` per unit; a `GraphEdge` per `depends_on` entry). Render the tick list itself (seq, action_tag, station, and the normalized prompt in a collapsed/pre block) so the transcript is inspectable. Handle the malformed-fixture edge per the spec's Edge cases section (a parse failure renders an error state, no wasm panic — `from_str` result matched, never unwrapped).
2. **`web/site/src/route.rs`**: add `#[route("/replay")] Replay {}` adjacent to the `/preview` variant inside the `#[layout(Shell)]` block; add `pub use pages::replay::Replay;` beside the existing page re-exports; add `"/replay".to_string()` to the STATIC vec in `Route::all_paths()` (next to `"/preview"` at line 124); extend the expected-paths array inside the `all_paths_covers_the_static_sections` test to include `/replay`.
3. **`web/site/src/pages/mod.rs`**: add `pub mod replay;` to the flat module list.

## Success / failure / edge paths

Success: the page compiles for wasm32, renders all three components plus the banner from the embedded fixture. Failure: malformed embedded JSON renders the error state (add a unit test parsing a truncated copy and asserting the error path value, not a panic). Edge: a fixture whose `units` is empty renders an empty-graph state without panicking (test it); a station name in the fixture that the site's embedded factory content no longer knows is rendered as plain text, never resolved against the content corpus (the spec's stale-content edge).

## Completion criteria (verify each from the unit worktree root)

1. `cargo test -p darkrun-site` exits 0 (includes the four route tests — `all_paths_are_unique_and_rooted` catches a malformed insertion — plus your new tests).
2. `cargo build -p darkrun-site --target wasm32-unknown-unknown` exits 0 (AC-15's check).
3. `cargo clippy -p darkrun-site --target wasm32-unknown-unknown -- -D warnings` exits 0.
4. Route registered in BOTH places → `grep -c '"/replay"' web/site/src/route.rs` reports >= 2 (the `#[route]` attribute and the `all_paths()` literal).
5. Components + banner + no-fetch → `grep -E 'StationStrip|StationPipeline|UnitGraph' web/site/src/pages/replay.rs` shows all three inside rsx; `grep -iE 'no.live.feed|no live feed' web/site/src/pages/replay.rs` matches; `grep -nE 'gloo|remote::|\.fetch\(' web/site/src/pages/replay.rs` returns nothing (AC-13's exact checks).
6. No new dependency edge → `cargo tree -p darkrun-site -e normal | grep -c darkrun-mcp` reports 0 (AC-14), and `git diff --name-only` for your commits does not include `web/site/Cargo.toml`.

## Files touched

`web/site/src/pages/replay.rs` (new), `web/site/src/route.rs`, `web/site/src/pages/mod.rs`. Nothing else.

## Out of scope

No CI changes (sibling unit), no fixture regeneration (the committed fixture is the sim-spine unit's output — consume it read-only), no darkrun-ui component changes, no web/app work, no fetch/live-feed capability of any kind.




## Each Unit has its own worktree — work in it

Every wave Unit is isolated on its own branch + worktree, forked off the station branch. Run that Unit's beat **inside its worktree** so its diff never tangles with another Unit's in-flight work; the manager lands each Unit back onto the station branch when it locks. Do **not** commit a Unit's work to the station branch yourself.

- `replay-route` → `/Users/jwaldrip/dev/src/github.com/jwaldrip/darkrun/.claude/worktrees/wiggly-gathering-spark/.darkrun/worktrees/darkrun-sim/units/build/replay-route` (branch `darkrun/darkrun-sim/units/build/replay-route`)





## The Pass loop — make → challenge → resolve

The Pass loop is adversarial on purpose: a single confident pass is exactly where LLM output is most often confidently wrong, so a second pass red-teams the first before anything locks.

- **make** — the worker produces the Unit's output against its completion criteria. Build the real thing, not a sketch.
- **challenge** — a second pass attacks what make produced: edge cases, missing handling, lazy assumptions. Assume the first pass was optimistic.
- **resolve** — reconcile make and challenge into a Unit that satisfies its completion criteria with the challenges answered.


**Reject routing.** Workers carry a pass-loop role: `self_reviewer` = verify, . A `build` worker produces and repairs; a `verify` worker only judges; a `plan` worker only designs. When a beat **rejects**, bounce back to the **nearest preceding `build` worker** (pass it as `next_worker` to `darkrun_unit_iterate`) — skip `verify`/`plan` beats on the way back, since they can't fix. An `advance` rolls forward to the next worker in order.



**Quality-gate verifier nonce.** This dispatch carries a one-time verifier token: **`affa8c653e282c3d71173554591ff479c8e28a574a322901d86efb62a18c486d`**. When you record a quality gate with `darkrun_quality_gate_record`, pass it as `nonce`. The engine refuses a gate result without the matching token — so a gate is only ever recorded as part of a real verification dispatch, never self-certified. Run the gate's command for real, then record the actual outcome with this nonce.


Run **only the `test_author` beat** this tick. When the beat finishes, **record it** with `darkrun_unit_iterate` — pass the `worker`, the `result` (`advance` or `reject`), and a `note`: on advance, what you did and what the next worker needs to know; on reject, why you bounced it (a reject without a reason is refused). That note becomes the next beat's handoff above. Then call `darkrun_tick`; the manager advances the loop or releases the next wave. A Unit is locked only after Resolve and its completion criteria pass.

A Unit gets a **bounded pass budget** — the manager escalates a Unit that can't converge within it to the operator rather than grinding forever. Don't paper over a stuck Unit to dodge the escalation; a Unit that needs more passes than the budget allows is a signal the spec, the scope, or the approach is wrong, and that's the operator's call to make.



## Done when

The `test_author` beat is complete for every Unit in this wave and its output is recorded. Then call `darkrun_tick`.

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