---
name: darkrun-show
description: Display a darkrun Run's state — Stations, Units, completion criteria, and Checkpoint status
---

Call `darkrun_run_show` (optional slug/id) for the Run's Station/phase/Checkpoint status, and `darkrun_unit_list { run, station }` for a Station's Units and their completion criteria. Present it lead-with-what's-next: current Station and phase → Units done/in-progress/blocked → the next Checkpoint. Offer `/darkrun:darkrun-pickup` to advance.
