---
name: darkrun-show
description: Display a darkrun Run's state — Stations, Units, completion criteria, and Checkpoint status
---

Call `darkrun_run_show` (optional slug/id). It raises the Run in the **darkrun desktop app** (the interactive surface) and returns the structured state. Summarize the state lead-with-what's-next: current Station and phase → Units done/in-progress/blocked → the next Checkpoint; use `darkrun_unit_list { run, station }` for a Station's Units and completion criteria. If the desktop app isn't open, tell the operator to run `darkrun serve`. Offer `/darkrun:darkrun-pickup` to advance.
