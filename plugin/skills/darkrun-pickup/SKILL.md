---
name: darkrun-pickup
description: Advance a darkrun Run — the factory manager returns the next concrete action across Stations, Workers, and Checkpoints
---

# Pick Up a Run

The **manager** is the engine of darkrun. It owns the cursor through the factory and decides what
happens next; you execute what it returns.

## Process

1. **Call `darkrun_run_next`.** With no Run specified it resumes the active Run; otherwise pass the
   Run slug/id. The manager walks the three-track priority — **drift → feedback → run** — and the
   Station phase machine (**Elaborate → Execute → Review → Checkpoint**), then returns a structured
   **next action**.

2. **Do exactly what it returns.** The action tells you which Station and which Worker
   (Make → Challenge → Resolve), or to run Explorers, decompose into Units, run Reviewers, or hold
   at a Checkpoint. Follow it verbatim — don't improvise the sequence.

3. **Checkpoints.** When the manager surfaces a Checkpoint:
   - `auto` — it advances itself; nothing to do.
   - `ask` — confirm with the user, then `/darkrun:darkrun-checkpoint` to record the decision.
   - `external` / `await` — the Run holds until the external signal/decision arrives.

4. **Loop.** After completing the returned action, call `darkrun_run_next` again. Keep going until
   the manager reports the Run complete or hands control back to you.

Use `/darkrun:darkrun-show` any time to inspect Stations, Units, and completion criteria.
