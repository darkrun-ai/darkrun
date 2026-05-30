---
name: darkrun-statusline
description: Install, remove, or preview the darkrun Claude Code status line — a one-line station/phase indicator for the active Run
---

Manage the Run status line through the CLI:

- `darkrun statusline install` (`--global` for `~/.claude`) — wire Claude Code's `statusLine` to darkrun, saving the existing line as a restorable fallback. Inside a plugin install, pass `--command "${CLAUDE_PLUGIN_ROOT}/bin/darkrun statusline"`.
- `darkrun statusline uninstall` (`--global`) — restore the previous line.
- Preview without installing: `echo '{"workspace":{"current_dir":"'$PWD'"}}' | darkrun statusline`.

See `darkrun statusline --help` for details.
