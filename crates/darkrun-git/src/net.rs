//! Network-operation guards: a hard wall-clock deadline and non-interactive
//! credential behavior for push/fetch.
//!
//! The predecessor learned this the hard way (its #333): an unresponsive remote
//! — a hung TCP connection, a proxy black hole, an SSH key waiting on a
//! passphrase — wedges the engine tick indefinitely unless every network git
//! operation carries a deadline and every credential path is non-interactive.
//! Its `GIT_NETWORK_TIMEOUT_MS = 30s` + `GIT_TERMINAL_PROMPT=0` env are ported
//! here for the in-process gitoxide transport:
//!
//! - [`with_deadline`] runs a blocking network closure on a worker thread and
//!   abandons it past the deadline (the engine moves on; the OS reaps the
//!   socket). gitoxide's blocking transport has no native deadline, so the
//!   thread boundary IS the timeout.
//! - [`ensure_noninteractive`] sets the process env that suppresses every
//!   interactive credential path (terminal prompts, SSH passphrase prompts) so
//!   a missing credential FAILS fast instead of hanging — called once at
//!   engine boot.

use crate::error::{GitError, Result};

/// The default network deadline, overridable via `DARKRUN_GIT_TIMEOUT_SECS`
/// (tests set it low; `0`/garbage falls back to the default).
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// The effective network deadline.
pub fn network_deadline() -> std::time::Duration {
    let secs = std::env::var("DARKRUN_GIT_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    std::time::Duration::from_secs(secs)
}

/// Run `op` (a blocking network operation) under the wall-clock deadline.
///
/// On timeout the worker thread is **abandoned** — it holds no engine locks
/// (push/fetch open their own repo handle) and the process-wide non-interactive
/// env guarantees it isn't waiting on a human, so it dies with its socket. The
/// caller gets a clean, actionable error instead of a wedged tick.
pub fn with_deadline<T, F>(label: &str, op: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let deadline = network_deadline();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(format!("darkrun-net-{label}"))
        .spawn(move || {
            // The receiver may be gone (timed out) — a send failure is fine.
            let _ = tx.send(op());
        })
        .map_err(|e| GitError::Gix(format!("could not spawn network worker: {e}")))?;
    match rx.recv_timeout(deadline) {
        Ok(result) => result,
        Err(_) => Err(GitError::Gix(format!(
            "network operation '{label}' timed out after {}s (origin unreachable or \
             credential prompt suppressed) — the engine continues; re-sync on a later tick",
            deadline.as_secs()
        ))),
    }
}

/// Make every credential path non-interactive for THIS process — called once at
/// engine boot. A missing credential then fails fast instead of prompting:
///
/// - `GIT_TERMINAL_PROMPT=0` — git-family tooling never prompts on the tty.
/// - `GIT_ASKPASS`/`SSH_ASKPASS` → `true` (exits 0 with empty output) and
///   `SSH_ASKPASS_REQUIRE=force` — passphrase prompts resolve empty immediately.
/// - `GIT_SSH_COMMAND="ssh -oBatchMode=yes"` (only when unset) — the spawned
///   `ssh` refuses interactive auth outright.
///
/// Existing values are respected (a user who configured a real askpass keeps
/// it). Safe to call repeatedly.
pub fn ensure_noninteractive() {
    let set_if_unset = |k: &str, v: &str| {
        if std::env::var_os(k).is_none() {
            std::env::set_var(k, v);
        }
    };
    set_if_unset("GIT_TERMINAL_PROMPT", "0");
    set_if_unset("GIT_ASKPASS", "true");
    set_if_unset("SSH_ASKPASS", "true");
    set_if_unset("SSH_ASKPASS_REQUIRE", "force");
    set_if_unset("GIT_SSH_COMMAND", "ssh -oBatchMode=yes");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_returns_the_op_result_when_fast() {
        let out = with_deadline("fast", || Ok::<_, GitError>(42)).expect("fast op completes");
        assert_eq!(out, 42);
    }

    #[test]
    fn deadline_propagates_op_errors() {
        let err = with_deadline::<(), _>("err", || {
            Err(GitError::Gix("remote said no".into()))
        })
        .unwrap_err();
        assert!(err.to_string().contains("remote said no"));
    }

    #[test]
    fn deadline_abandons_a_hung_op() {
        // A 1-second deadline (via env) abandons an op that sleeps past it.
        std::env::set_var("DARKRUN_GIT_TIMEOUT_SECS", "1");
        let started = std::time::Instant::now();
        let err = with_deadline::<(), _>("hung", || {
            std::thread::sleep(std::time::Duration::from_secs(20));
            Ok(())
        })
        .unwrap_err();
        std::env::remove_var("DARKRUN_GIT_TIMEOUT_SECS");
        assert!(err.to_string().contains("timed out"), "{err}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "returned promptly, not after the op's 20s"
        );
    }

    #[test]
    fn noninteractive_env_sets_only_unset_keys() {
        std::env::set_var("GIT_TERMINAL_PROMPT", "1"); // user explicitly chose
        std::env::remove_var("SSH_ASKPASS_REQUIRE");
        ensure_noninteractive();
        assert_eq!(std::env::var("GIT_TERMINAL_PROMPT").unwrap(), "1");
        assert_eq!(std::env::var("SSH_ASKPASS_REQUIRE").unwrap(), "force");
        std::env::remove_var("GIT_TERMINAL_PROMPT");
    }
}
