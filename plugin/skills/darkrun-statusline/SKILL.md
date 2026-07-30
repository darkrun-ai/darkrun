---
name: darkrun-statusline
description: Install, remove, or preview the darkrun Claude Code status line — a one-line station/phase indicator for the active Run
---

Manage the Run status line through the CLI:

- `darkrun statusline install` (`--global` for `~/.claude`) — wire Claude Code's `statusLine` to darkrun, saving the existing line as a restorable fallback. It writes an ABSOLUTE path to the launcher it resolved, because Claude Code runs the status line with whatever PATH it has: a bare `darkrun` that is not on that PATH produces a command-not-found and a silently BLANK status line. Override with `--command "<command>"` only when you know better; install warns if the command it wrote does not resolve to a program.
- `darkrun statusline uninstall` (`--global`) — restore the previous line.
- Preview without installing: `echo '{"workspace":{"current_dir":"'$PWD'"}}' | darkrun statusline`.
- A blank status line in one project is almost always this: check `.claude/settings.json` there for a bare `darkrun statusline` command and re-run install to rewrite it.

See `darkrun statusline --help` for details.
