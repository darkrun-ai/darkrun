---
name: darkrun-statusline
description: Install, remove, or preview the darkrun Claude Code status line — a one-line station/phase indicator for the active Run
---

# Status Line

darkrun ships a Claude Code status line that shows, at a glance, where the active Run sits in its
factory:

```
darkrun · add-healthcheck ●●◉○○○ build ❯ execute · 3/8 units
```

- the **darkrun** wordmark (dark bold · run regular), then the Run slug
- the **station pipeline**: `●` complete · `◉` active · `○` pending
- the active **station**, a flow mark (`❯` running · `⊘` gated at a non-auto Checkpoint), and the
  color-coded **phase** (elaborate=grey, execute=cyan, review=yellow, checkpoint=magenta)
- a unit aggregate: completed / total

With no active Run (no `.darkrun/`, or outside a project) it prints nothing, so Claude Code shows
whatever line you had before — it's purely additive.

## Install

```bash
darkrun statusline install            # wires the project .claude/settings.json
darkrun statusline install --global   # wires ~/.claude/settings.json
```

This points Claude Code's `statusLine` at `darkrun statusline` and saves your existing line as a
restorable fallback (`.darkrun/statusline-fallback.json`, or `~/.darkrun/` for `--global`). Inside
a plugin install, pass `--command "${CLAUDE_PLUGIN_ROOT}/bin/darkrun statusline"`.

## Remove

```bash
darkrun statusline uninstall          # or --global
```

Restores the status line you had before (or removes the key if you had none).

## Preview

Pipe a workspace dir to see the rendered line without installing:

```bash
echo '{"workspace":{"current_dir":"'$PWD'"}}' | darkrun statusline
```
