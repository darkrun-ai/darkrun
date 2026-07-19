
> **Run** `darkrun-sim` · **Phase** `save_wip`


# Save Work in Progress — `darkrun/darkrun-sim/main`

You have **uncommitted work** in the project tree. The manager will not advance the run with loose changes on `darkrun/darkrun-sim/main` — commit them first, then retick. This is purely mechanical: no human intervention needed.


**Contract**

- Do exactly the work this action describes — no more, no less. Don't skip ahead to a later phase.
- Treat the locked artifact as the source of truth. Read it before you act; never silently rewrite a locked decision.
- Every claim you make must be backed by something you actually ran, read, or wrote. No assumed results.
- Be specific and committed. **No placeholders** — a `TBD`, `similar to …`, `add error handling`, `etc.`, or `…` is a hole, not a decision; name the actual, checkable condition. **No hedging** — when you report work done, use a verb of completed action (`added`, `implemented`, `fixed`), never `should`, `seems`, `probably`, `might`, or `looks like`. Hedging is the tell of unfinished work.
- When the action is finished, record your output where the station expects it, then call `darkrun_tick` again for the next instruction. The manager — not you — decides what comes next.

## Why the engine won't commit this for you

The engine commits its own `.darkrun/` bookkeeping on every tick — but it never authors **your** commits. You know what you just did; a generic engine "wip" dump can't tell the story of the work. Commit messages are part of the record the run leaves behind.

## Uncommitted paths

- `desktop/src/review.rs`
- `desktop/src/wire.rs`
- `desktop/tests/wire_payloads.rs`


## What to do

1. **Group related changes** into separate, coherent commits — one logical step each, not a single catch-all dump.
2. **Write messages that explain the why** of each change, not just the what.
3. Commit everything listed above on `darkrun/darkrun-sim/main` (`git add … && git commit …`).
4. Re-run `darkrun_tick`. The manager re-checks the tree and resumes the run.

If a listed file is scratch output you never meant to keep, delete it (or gitignore it) instead of committing it — the gate clears either way.

## Done when

The project tree is clean apart from the engine's own `.darkrun/` state, and `darkrun_tick` advances past this action.