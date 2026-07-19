
> **Run** `darkrun-sim` · **Station** `build` · **Phase** `user_gate`

> Eliminates: _implementation-defects_


# User gate — `build` spec

The review work for station **build** is done: the spec is written, the adversarial reviewers have had their pass, and the review brief is recorded. Before a single Unit is manufactured, the **operator** reviews the station and clears it — this is the pre-execution gate, the cheapest place to catch a wrong direction.


**Contract**

- Do exactly the work this action describes — no more, no less. Don't skip ahead to a later phase.
- Treat the locked artifact (`code`) as the source of truth. Read it before you act; never silently rewrite a locked decision.
- Every claim you make must be backed by something you actually ran, read, or wrote. No assumed results.
- Be specific and committed. **No placeholders** — a `TBD`, `similar to …`, `add error handling`, `etc.`, or `…` is a hole, not a decision; name the actual, checkable condition. **No hedging** — when you report work done, use a verb of completed action (`added`, `implemented`, `fixed`), never `should`, `seems`, `probably`, `might`, or `looks like`. Hedging is the tell of unfinished work.
- When the action is finished, record your output where the station expects it, then call `darkrun_tick` again for the next instruction. The manager — not you — decides what comes next.

## The decision happens in the desktop review surface

This tick **raised the desktop app** pointed at this gate — that is the operator's review surface, and the only place this gate is decided. The operator reads the spec/brief there and either approves the wave or returns feedback.

**Do NOT ask the operator inline.** No `AskUserQuestion`, no chat prompt, no improvised approve/reject question — the gate lives in the desktop, not the transcript. And do not advance the run yourself.

- The operator **approves** in the desktop → the gate clears and the next tick releases the manufacture wave.
- The operator **returns feedback** → it lands as a fix track; address it, then the gate re-opens for their re-decision.

If the desktop did not come up, call `darkrun_run_inspect` to raise it again — never substitute an inline question for the gate.

## Done when

The operator has cleared the gate via `darkrun_checkpoint_decide` (the desktop's Approve button calls it). Until then, this gate holds. Call `darkrun_advance` to re-check — a held gate is not a wedge; it is waiting on a human at the desktop.