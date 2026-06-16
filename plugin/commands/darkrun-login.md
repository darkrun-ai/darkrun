---
description: Sign in to enable REMOTE access to your runs — control and watch them from the web app or your phone, not just this machine
argument-hint: [github|gitlab]
---

Enable **remote** access to your darkrun runs.

Local runs already work with no login — the desktop app reaches the engine over loopback. **Remote** access (watching/controlling a run from app.darkrun.ai or a phone, through the relay) requires signing in, because the relay must know which account a session belongs to.

The login signs you in at the darkrun web app (`$ARGUMENTS` = `github` or `gitlab`; you can link both to one account) and stores the **Firebase ID token** the engine presents when it dials the relay (read as `DARKRUN_RELAY_TOKEN`). The relay verifies that token and binds the session to your account. With a relay configured (`DARKRUN_RELAY_URL`) plus this token, a run becomes reachable remotely; without it, runs stay local-only.

Check a run's reach with `darkrun_run_inspect` — its `reachability` lists the local candidate (always) and the relay candidate (once remote is enabled).

