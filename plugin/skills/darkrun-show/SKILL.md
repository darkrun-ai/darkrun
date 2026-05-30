---
name: darkrun-show
description: Display a darkrun Run's state — Stations, Units, completion criteria, and Checkpoint status
---

# Show a Run

## Process

1. **Run overview.** Call `darkrun_run_show` (optionally with a Run slug/id) for the Run's title,
   factory, current Station, phase, and Checkpoint status.

2. **Units.** Call `darkrun_unit_list { run, station }` for the Units at a Station with their
   completion criteria and status (and the dependency DAG / ready-set).

3. **Present it cleanly.** Summarize as: current Station and phase → Units done / in-progress /
   blocked → the next Checkpoint and its kind. Lead with where the Run is and what's next, not a
   wall of raw state. Offer `/darkrun:darkrun-pickup` to advance.
