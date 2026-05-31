//! Launching the darkrun desktop app — the only interactive surface the engine
//! brings up (never a browser). `darkrun_show` calls [`spawn`] to open the app
//! pointed at the running engine, when none is already connected.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The desktop binary name for this platform.
fn bin_name() -> &'static str {
    if cfg!(windows) {
        "darkrun-desktop.exe"
    } else {
        "darkrun-desktop"
    }
}

/// Locate the `darkrun-desktop` binary: an explicit `DARKRUN_DESKTOP` path, then
/// a sibling of the running engine binary (plugin installs ship them together),
/// then the repo's `target/{debug,release}` (dev builds). `None` when not found.
pub fn find(repo_root: &Path) -> Option<PathBuf> {
    let name = bin_name();
    if let Ok(p) = std::env::var("DARKRUN_DESKTOP") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(sib) = exe.parent().map(|d| d.join(name)) {
            if sib.is_file() {
                return Some(sib);
            }
        }
    }
    for prof in ["debug", "release"] {
        let p = repo_root.join("target").join(prof).join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Spawn the desktop app (detached) pointed at the engine `port`. It is launched
/// **unpinned** (`DARKRUN_SESSION_ID` cleared) so it opens the run-browser home,
/// whose `current`-focus poller then navigates to the run the engine just
/// raised. Returns the launched binary path, or `None` if it couldn't be found
/// or spawned.
pub fn spawn(repo_root: &Path, port: u16) -> Option<PathBuf> {
    let bin = find(repo_root)?;
    Command::new(&bin)
        .env("DARKRUN_PORT", port.to_string())
        .env_remove("DARKRUN_SESSION_ID")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
        .map(|_| bin)
}

#[cfg(test)]
mod tests {
    use super::*;

    // One sequential test: DARKRUN_DESKTOP is process-global, so mutating it in
    // parallel tests would race.
    #[test]
    fn find_resolves_env_then_target() {
        // Explicit env path wins.
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join(bin_name());
        std::fs::write(&fake, b"#!/bin/sh\n").unwrap();
        std::env::set_var("DARKRUN_DESKTOP", &fake);
        assert_eq!(find(dir.path()).as_deref(), Some(fake.as_path()));

        // With no env override, falls through to repo target/<profile>.
        std::env::remove_var("DARKRUN_DESKTOP");
        let repo = tempfile::tempdir().unwrap();
        let target = repo.path().join("target").join("release");
        std::fs::create_dir_all(&target).unwrap();
        let bin = target.join(bin_name());
        std::fs::write(&bin, b"x").unwrap();
        assert_eq!(find(repo.path()), Some(bin));
    }
}
