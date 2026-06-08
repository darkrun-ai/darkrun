---
name: darkrun-new
description: Start a new darkrun Run — describe what you want to accomplish and the factory manager scaffolds a right-sized lifecycle for it
---

Capture the intent cleanly, then right-size the lifecycle to the work. If the request is vague, prelaborate it into a crisp description first (ask via `AskUserQuestion`). Call `darkrun_factory_list` if the factory isn't obvious, then `darkrun_run_new { slug, title, factory, mode, size }`.

Right-size with `--size full|quick|bugfix|refactor` (the station plan):

- `full` (default) — the whole Frame→Harden line.
- `quick` — build + prove, for small self-contained work.
- `bugfix` — specify + build + prove.
- `refactor` — shape + build + prove.

Pick the review `mode` (orthogonal to size) from how much oversight the work needs:

- `solo` (default) — each station asks for local review before advancing.
- `team` — each station opens a PR/MR the team reviews and merges (`darkrun/<slug>/<station>` → `darkrun/<slug>/main`); the manager advances when you merge it. Needs a hosting client (`gh`/`glab` on PATH, configured via `/darkrun:darkrun-setup`); without one, a station gate falls back to a manual review gate you resolve by hand.
- `dark` — pre-elaborate up front, then run without stopping for review.

Drive the loop with `/darkrun:darkrun-resume`.
