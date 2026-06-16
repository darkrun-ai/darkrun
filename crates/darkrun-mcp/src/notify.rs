//! Local OS notifications — the local half of "notify as the engine ticks".
//!
//! For LOCAL sessions, the host fires a native notification into the OS
//! notification center when a run reaches a moment that needs the operator (a
//! gate) — so you're told even when the desktop window isn't focused. The REMOTE
//! half (FCM push to an account's registered devices) is sent separately when a
//! Firebase project is configured; the two are complementary.
//!
//! Best-effort + dependency-free: it shells out to the platform notifier
//! (`osascript` on macOS, `notify-send` on Linux), so a missing notifier is a
//! silent no-op. OPT-IN via `DARKRUN_NOTIFY=1` so it never fires in tests or on
//! headless CI; the message building is pure and unit-tested regardless.

/// Whether local notifications are enabled (`DARKRUN_NOTIFY=1`). Off by default
/// so tests/headless never raise a desktop notification.
pub fn enabled() -> bool {
    std::env::var("DARKRUN_NOTIFY").ok().as_deref() == Some("1")
}

/// The notification title + body for a run reaching an operator gate. Delegates
/// to the shared [`darkrun_api::notify::gate_message`] so the LOCAL OS
/// notification and the REMOTE push read identically.
pub fn gate_message(run: &str, station: &str) -> (String, String) {
    darkrun_api::notify::gate_message(run, station)
}

/// Fire a native OS notification, best-effort. A no-op unless [`enabled`].
#[cfg(not(tarpaulin_include))] // spawns the OS notifier — irreducible process I/O
pub fn fire(title: &str, body: &str) {
    if !enabled() {
        return;
    }
    let _ = spawn_notifier(title, body);
}

/// Notify that `run` has reached an operator gate on `station`.
pub fn on_gate(run: &str, station: &str) {
    let (title, body) = gate_message(run, station);
    fire(&title, &body);
}

/// Spawn the platform notification command. Silent on platforms without one.
#[cfg(not(tarpaulin_include))]
fn spawn_notifier(title: &str, body: &str) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let mut cmd;
    #[cfg(target_os = "macos")]
    {
        // AppleScript: display a notification with our title.
        let script = format!(
            "display notification {} with title {}",
            applescript_quote(body),
            applescript_quote(title),
        );
        cmd = Command::new("osascript");
        cmd.arg("-e").arg(script);
    }
    #[cfg(target_os = "linux")]
    {
        cmd = Command::new("notify-send");
        cmd.arg(title).arg(body);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        // No supported notifier — nothing to do.
        let _ = (title, body);
        return Ok(());
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        Ok(())
    }
}

/// Quote a string as an AppleScript string literal (escape `\` and `"`).
#[cfg(target_os = "macos")]
fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_message_names_the_run_and_capitalizes_the_station() {
        let (title, body) = gate_message("quiet-canyon", "build");
        assert_eq!(title, "darkrun · quiet-canyon");
        assert_eq!(body, "Build needs your decision.");
    }

    #[test]
    fn gate_message_handles_an_empty_station() {
        let (_t, body) = gate_message("r", "");
        assert_eq!(body, "A checkpoint needs your decision.");
    }
}
