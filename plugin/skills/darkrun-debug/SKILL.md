---
name: darkrun-debug
description: Admin recovery for wedged Runs — preview the manager cursor, force a Station complete, set engine-managed fields, reset drift, or patch a feedback record. Every mutation requires explicit user confirmation.
---

Recovery for Runs the manager's normal loop can't clear. Call `darkrun_debug` with `run` + `op` (+ a `reason` on every mutating op), and do exactly what it returns. The tool confirms each mutation with the user before touching `.darkrun/` — surface that prompt and never bypass or auto-retry it. Run the read-only `preview_cursor` op before and after any change.

For a destructive Station/Run wipe use `/darkrun:darkrun-reset` instead.
