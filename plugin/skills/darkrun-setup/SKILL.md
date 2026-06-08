---
name: darkrun-setup
description: Configure darkrun for this project — auto-detect VCS, hosting, CI/CD, and default branch, confirm with the user, and write .darkrun/settings.yml
---

Call `darkrun_setup` to auto-detect the project environment (VCS, hosting, CI/CD, default branch) and available MCP providers, present what it found to the user via `AskUserQuestion`, adjust, then write `.darkrun/settings.yml`. It's additive and idempotent — only the confirmed changes are written. Then suggest `/darkrun:darkrun-new`.
