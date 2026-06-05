# Predecessor bug audit — does darkrun share them?

Audited the predecessor engine's recorded bug reports (from `~/Downloads/`) against
darkrun source: 11 distinct engine bugs across 4 reports. Each is classified as
**FIXED** (darkrun shared it — now fixed), **immune** (darkrun's design prevents
it — verified + regression-tested), or **backstopped** (caught by an existing
guard). Every row has a test.

> Source reports: `haiku-engine-bugs-admin-portal-reimagine.md` (BUG-1…6),
> `haiku-drift-loop-bug-5.0.3.md` (drift A/B), `haiku-pick-design-direction-bug-2026-05-18.md`
> (BUG-9/10), `bug-spa-selection-no-chat-breadcrumb.md` (BUG-11).

| # | Predecessor bug | darkrun | Where |
|---|---|---|---|
| **BUG-3** | 0-byte/`touch`ed output passes the existence check (`existsSync` only); empty file reads "stable" to drift | **FIXED** — `missing_outputs` now requires a regular, non-empty file (`output_present`) | position.rs |
| **BUG-1** | CI-deferral attempt counter keyed per-UNIT → a gate defers on its FIRST failure (inherits the unit's count) | **immune** — `attempts` keyed per-gate-name; only `env_blocked` defers, never a `fail` | units.rs |
| **Drift A** | sweep diffs via `git log` on a worktree-prefixed path → `commits:[]` sticky false positive → infinite `drift_detected` | **immune** — drift is pure content-hash (zero git); no `commits` field, no path-prefix bug | drift.rs |
| **Drift B** | `target_invalidates` never clears `approvals.<role>` on FB close → witness never refreshes → loops forever | **immune** — `close_with_reply` actually removes the invalidated roles from `reviews`/`approvals`; restamp-on-detect fires once | feedback.rs · drift.rs |
| **BUG-6** | non-code finding reaches a builder that can only edit-or-reject → loops to the bolt cap, never closes | **immune** — terminal non-code routes (`Answered`, `NonActionable`) settable directly | feedback.rs |
| **BUG-9** | interactive tool didn't write its declared manifest file → discovery gate looped on file-existence | **immune/backstopped** — darkrun doesn't couple a gate to a tool-written file; the deadlock guard escalates any stuck action after 4 ticks | sessions.rs · deadlock.rs |
| **BUG-10** | second tool call replayed the same cached selection without re-opening the picker | **immune** — each `create_*` mints a fresh incrementing session id; no stale replay | sessions.rs |
| **BUG-5** | `report` silently no-ops (returns success) without a Sentry DSN → findings dropped on dev builds | **immune** — `report` always writes a durable `.darkrun/reports/<id>.md`; never a dead sink | meta.rs |
| **BUG-4** | seal wrote frontmatter via a raw non-commit → auto-push skipped → origin stale, CI ran old code | **mitigated** — darkrun's land is a real merge commit (state is gitignored), so the bug can't occur; the seal prompt now surfaces `branch_status` (ahead/diverged) so the operator knows origin still needs a push | position.rs · lifecycle.rs · sealed.md |
| **BUG-2** | intent-scope gates re-emit from frozen unit specs after code relocates (no `superseded`) → loop | **covered** — `env_blocked → deferred_to_ci` (a gate that can't run locally auto-defers); deadlock guard backstops | units.rs |
| **BUG-11** | out-of-band SPA studio/mode selection leaves no breadcrumb → agent mistakes legit fields for silent defaults, breaks autopilot | **immune by design** — `run_start` takes factory+mode as explicit args (no silent defaults); the tick reflects resolved state | position.rs |

## The one real shared bug

Only **BUG-3** was a genuine shared defect (the output-existence gate trusted
`.exists()` and would pass a 0-byte file) — fixed. Everything else darkrun's
architecture already prevented:

- **Drift was the big one.** The predecessor's most severe report (an
  intent-blocking infinite drift loop across two engine versions) came from two
  causes — git-log path resolution and `invalidates` not clearing approvals — that
  the darkrun drift rebuild structurally eliminates: content-hash diffing (no git),
  restamp-on-detect (fires once), and a close that actually clears the stamps. A
  regression test mirrors the exact repro and proves the loop can't recur.
- **The deadlock/churn halt** is darkrun's general backstop for the whole class of
  "engine returns the same unmet signal forever" bugs (BUG-2, BUG-9): any action
  stuck 4× with no progress escalates to the operator instead of looping.
- **No dead sinks, no silent defaults.** `report` always writes locally (BUG-5);
  `run_start` requires explicit factory+mode (BUG-11).

*Companion to `engine-parity-gaps.{csv,md}`. Audit date: 2026-06-05.*
