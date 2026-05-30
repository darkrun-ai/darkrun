---
name: darkrun-reset
description: Wipe one Station of a Run (its Units, outputs, artifacts, decomposition, feedback, and branch) so the manager re-enters it from scratch — or reset/archive the whole Run. Other Stations stay untouched.
---

Wipe a Station so the manager re-enters it from its spec phase; other Stations, their approvals, and the Run's main history stay put. Call `darkrun_run_reset { run, station }` (omit `station` to reset the whole Run; `darkrun_run_archive { run }` to retire it). The tool confirms and lists exactly what will be deleted before wiping. After the user confirms, call `darkrun_run_next` to re-enter the Station.

For surgical recovery without a wipe use `/darkrun:darkrun-debug`; for a few specific issues use `darkrun_feedback_create`.
