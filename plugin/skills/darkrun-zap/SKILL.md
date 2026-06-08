---
name: darkrun-zap
description: Zero-ceremony single-Unit execution — run one task straight through a Station's Worker loop with no Run, no decomposition, and no state written under .darkrun/
---

Stateless single-task execution — no Run record, no decomposition, nothing written under `.darkrun/`. Call `darkrun_zap { task, factory?, station? }` and follow the returned `message` verbatim (it carries the Worker sequence, per-Worker subagent prompts, and the run/verify/commit procedure). If it returns a `*_not_found` error, surface the `valid_*` list via `AskUserQuestion` and retry.

For multi-Station or tracked work use `/darkrun:darkrun-new` (right-size it with `--size quick` for small self-contained work).
