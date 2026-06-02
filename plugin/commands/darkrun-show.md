---
description: Show a darkrun Run's state — Stations, Units, completion criteria, and Checkpoint status
argument-hint: [run slug]
---

Show the darkrun Run state for `$ARGUMENTS`.

Call `darkrun_run_show` (and then `darkrun_unit_list`). **Omit the `slug` when `$ARGUMENTS` is empty** — do not go hunting for it. `darkrun_run_show` infers the run itself, in order: the current `darkrun/<slug>/…` branch you're standing in, then the active-run pointer, then the sole run. Pass `slug` only when `$ARGUMENTS` names one.

Lead with the current Station/phase and what's next. See the `darkrun-show` skill.
