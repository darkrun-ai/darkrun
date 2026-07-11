
> **Run** `darkrun-sim` · **Station** `specify` · **Phase** `fix_feedback`

> Eliminates: _ambiguity_


# Fix Feedback — `fb-06`

Open feedback preempts forward run progress. Something a reviewer or operator flagged is unresolved, and it routes to a **fix-worker** before the line moves on.


Dispatch one of **this station's** fix-workers — `builder`, `reconciler`, `validator` — the repairers specialized for this station's class of work.



**Contract**

- Do exactly the work this action describes — no more, no less. Don't skip ahead to a later phase.
- Treat the locked artifact (`spec.md`) as the source of truth. Read it before you act; never silently rewrite a locked decision.
- Every claim you make must be backed by something you actually ran, read, or wrote. No assumed results.
- Be specific and committed. **No placeholders** — a `TBD`, `similar to …`, `add error handling`, `etc.`, or `…` is a hole, not a decision; name the actual, checkable condition. **No hedging** — when you report work done, use a verb of completed action (`added`, `implemented`, `fixed`), never `should`, `seems`, `probably`, `might`, or `looks like`. Hedging is the tell of unfinished work.
- When the action is finished, record your output where the station expects it, then call `darkrun_tick` again for the next instruction. The manager — not you — decides what comes next.


## This fix has its own worktree — work in it

The repair is isolated on its own branch + worktree, forked off the station branch: **`/Users/jwaldrip/dev/src/github.com/jwaldrip/darkrun/.claude/worktrees/wiggly-gathering-spark/.darkrun/worktrees/darkrun-sim/fixes/specify/fb-06`**. Make the fix **inside that worktree** so its diff never tangles with in-flight unit work; the manager lands it back onto the station branch when you resolve the feedback. Don't commit the fix to the station branch yourself.


## What to do

1. **Read the feedback item** `fb-06` in full (station `specify`). Understand the actual complaint, not your guess at it.
2. **Reproduce or locate** the problem in the real artifact. Don't fix what you can't first see.
3. **Make the smallest correct change** that resolves it. Don't rewrite unrelated work to scratch an itch.
4. **Re-verify** against the feedback's criteria — the fix isn't done until the original concern is demonstrably gone.
5. **Close the loop**: record what you changed and why on `fb-06`, and resolve it.

## Done when

`fb-06` is resolved with evidence, the artifact is corrected, and nothing else regressed. Then call `darkrun_tick` — if more feedback is open, the manager routes the next item; otherwise the run resumes.