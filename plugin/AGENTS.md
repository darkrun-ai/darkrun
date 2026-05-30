# darkrun

This project uses **darkrun** — a dark factory harness. darkrun runs as an MCP
server that drives a **Run** through a **Factory**'s **Stations** (Frame →
Specify → Shape → Build → Prove → Harden for the software factory), one
structured action at a time.

## How to work

The darkrun **manager** owns the cursor. You execute what it returns.

1. **At the start of each session**, call `darkrun_run_next` to load and advance
   the active Run (this harness has no auto-context hook, so you must call it
   yourself). To start a new Run, call `darkrun_run_start`.
2. **Do exactly what the returned action says.** Its rendered prompt carries the
   Station, Worker, and Checkpoint instructions. Each response ends with a
   **Harness note** describing how this harness differs from the default — honor
   it (run Units sequentially if there are no subagents, make decisions inline if
   there's no review UI, register Unit outputs explicitly since nothing is
   auto-tracked).
3. **Loop** — call `darkrun_run_next` again after each step until the manager
   reports the Run complete.
4. **Checkpoints** — when a Station holds at a gate, surface the decision and
   record it with `darkrun_checkpoint_decide` (`approved: true` to advance,
   `approved: false` with `feedback` to route rework).

Durable Run state lives under `.darkrun/` and is harness-agnostic — a Run begun
in another tool resumes here from the same cursor.
