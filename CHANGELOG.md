# Changelog

All notable changes to darkrun are recorded here. Versions follow semver.

## 0.1.0 — unreleased

The first darkrun: a native Rust engine that drives Runs through a factory's
stations (Frame → Specify → Shape → Build → Prove → Harden for the software
factory), with a desktop review app and a Claude Code plugin.

- **Manager** — a pure-read cursor over `.darkrun/` state, walking the
  six-phase station machine (spec → review → manufacture → audit → reflect →
  checkpoint) across a three-track priority (drift → feedback → run).
- **Full action set** — validation (units-invalid, escalate, safe-repair),
  repair/rollback, external review, and the seal tail.
- **Objective verification** — surface-routed proof (`darkrun verify web`,
  `darkrun bench`) instead of eyeballed review.
- **Reflection** — durable run-level retrospectives.
- **Auto-tune** — run-start right-sizing (quick / bugfix / refactor / full).
- **Drift sweep** — detects mutated locked artifacts and self-heals on revert.
- **Multi-harness** — Claude Code, Cursor, Windsurf, Gemini CLI, OpenCode,
  Kiro, and Codex, each adapted from one capability registry.
