# Running darkrun in other harnesses

darkrun is an MCP server. It detects its host harness from `--harness <name>`
(or the `DARKRUN_HARNESS` env) and adapts its tools, instructions, and prompts
to that harness's capabilities — tool budget, parallel subagents, elicitation,
hooks, slash commands, and model tiers. The durable Run state under `.darkrun/`
is harness-agnostic, so a Run started in one harness resumes cleanly in another.

Valid harness values: `claude-code`, `cursor`, `windsurf`, `gemini-cli`,
`opencode`, `kiro`, `codex`.

## Where each config goes

| Harness | Config | In this repo |
|---|---|---|
| Claude Code | bundled `plugin/.mcp.json` (`--harness` defaults to `claude-code`) | shipped |
| Cursor | `plugin/.cursor-plugin/plugin.json` | shipped |
| Gemini CLI | `plugin/gemini-extension.json` (+ `GEMINI.md`, `.toml` commands) | shipped |
| OpenCode | `plugin/opencode.json` | shipped |
| Windsurf | `~/.codeium/windsurf/mcp_config.json` | sample: `windsurf/mcp_config.json` |
| Kiro | `.kiro/agents/darkrun.yaml` (or Settings UI) | sample: `kiro/darkrun.yaml` |
| Codex | `~/.codex/config.toml` (or project `.codex/config.toml`) | sample: `codex/config.toml` |

Each config launches `darkrun mcp --harness <name>`. The samples use
`npx -y darkrun` so they resolve the published per-arch binary regardless of
install location; swap in an absolute path to `bin/darkrun` if you prefer a
pinned local build.

## Known cross-harness limitations

Outside Claude Code, the hook-driven conveniences become manual:

- **No auto-context injection** — call `darkrun_run_next` yourself at the start
  of each session to load the active Run.
- **No automatic output tracking** — register a Unit's outputs explicitly.
- **No browser review UI** — review gates fall back to MCP elicitation (where
  the harness supports it) or an inline text decision.
- **No parallel subagents** on some harnesses — Units run sequentially.

The engine still drives the Run; you just do the bookkeeping the hooks would
have. Each `darkrun_run_next` response carries a "Harness note" spelling out
what applies to your harness.
