---
name: darkrun-quick
description: Quick single-Station Run — create a Run the manager auto-sizes to one Station, then advance it through that Station's phases
---

A quick task is an ordinary Run the manager right-sizes down to a single Station. Create it with `darkrun_run_start`, then drive it with `darkrun_run_next` until the Station's Checkpoint resolves — do exactly what each returned action says.

If the work clearly needs multiple Stations, use `/darkrun:darkrun-start` instead; if it's trivial and you want zero state, use `/darkrun:darkrun-zap`.
