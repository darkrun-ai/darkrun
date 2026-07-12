//! Launching the darkrun desktop app — the only interactive surface the engine
//! brings up (never a browser). `darkrun_show` calls [`spawn`] to open the app
//! pointed at the running engine, when none is already connected.
//!
//! Resolution mirrors how the `bin/darkrun` shim resolves the CLI:
//! - **Dev checkout** (the engine is running from a cargo workspace's
//!   `target/<profile>/`): on macOS, if the shipped "Darkrun AI" app is installed
//!   it is **auto-detected and driven** (so a dev-checkout engine can smoke-test
//!   the released app). Otherwise, or with `DARKRUN_DESKTOP=local`, the local
//!   `target/<profile>/darkrun-desktop` is used, **built on demand** for the host
//!   arch if it isn't built yet.
//! - **Installed plugin**: the per-arch sub-package ships `darkrun-desktop` next
//!   to `darkrun`, so it's a sibling of the running engine binary.
//! - `DARKRUN_DESKTOP=<path>` overrides everything; `DARKRUN_DESKTOP=installed`
//!   (macOS) opens the shipped Mac App Store / TestFlight app by bundle id via
//!   LaunchServices, so a dev-checkout engine can drive the released app.
//!
//! ## macOS: launch via LaunchServices, not a bare `exec`
//!
//! The MCP server is itself spawned by the harness (Claude Code) in a process
//! context that is *detached from the Aqua GUI session*. A GUI app `exec`'d
//! directly from there cannot reach the WindowServer and AppKit simply `exit()`s
//! it — so `Command::spawn().is_ok()` reports success (fork/exec worked) while the
//! window never appears and the process is gone a moment later. The fix is to hand
//! the launch to **LaunchServices** via `open`, which starts the app *in* the
//! login GUI session regardless of who asked. `open` needs an `.app` bundle, so we
//! materialize a tiny wrapper (Info.plist + a symlink to the real binary) next to
//! the binary on demand. `open --stdout/--stderr` captures the app's output to a
//! log so a launch is never silent again.

use darkrun_http::Presence;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// How a human review gate should be SURFACED — the notify-and-await decision.
///
/// Picks between holding (the human surface is already live, so just `await` the
/// decision over the broadcast channel) and launching (no confident live surface,
/// so open the desktop app as before). See [`surface_mode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceMode {
    /// A human surface is already live (or strongly believed to be) — do NOT
    /// relaunch the app; hold the gate via the await loop and let the live mirror
    /// / push carry the raised session to the connected client.
    Await,
    /// No confident live surface — open the desktop app (the historical behavior).
    Launch,
}

/// The gate-surfacing decision: given how confident we are that a human surface
/// is already live, choose whether to hold (`Await`) or open the app (`Launch`).
///
/// Pure, so the policy is exhaustively testable in isolation from the launch
/// machinery:
/// - `presence.is_present()` (a review client is connected, or within the
///   [`darkrun_http::Presence::Lost`] grace window) → [`SurfaceMode::Await`].
///   A connected client gets the raised session pushed to it; relaunching would
///   only spawn a redundant window.
/// - else if `acked` (a device confirmed a push receipt for this session) →
///   [`SurfaceMode::Await`]. A device proved it received the gate notification,
///   so hold for the decision rather than opening a local window.
/// - else → [`SurfaceMode::Launch`]: no live client and no acked device, so open
///   the desktop app — the original behavior.
pub fn surface_mode(presence: Presence, acked: bool) -> SurfaceMode {
    if presence.is_present() || acked {
        SurfaceMode::Await
    } else {
        SurfaceMode::Launch
    }
}

/// Where a launched app's stdout/stderr is captured, so a failed launch leaves a
/// trace instead of vanishing silently. Lives under the project's state dir.
fn log_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".darkrun").join("desktop.log")
}

/// Open the launch log for append (creating `.darkrun/` if needed), for wiring a
/// child's stdout/stderr to it.
fn open_log(repo_root: &Path) -> Option<std::fs::File> {
    let path = log_path(repo_root);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
}

/// A child's stdout/stderr wired to the launch log, or null if it can't be opened.
fn log_stdio(repo_root: &Path) -> (Stdio, Stdio) {
    match open_log(repo_root) {
        Some(f) => match f.try_clone() {
            Ok(f2) => (Stdio::from(f), Stdio::from(f2)),
            Err(_) => (Stdio::from(f), Stdio::null()),
        },
        None => (Stdio::null(), Stdio::null()),
    }
}

/// The desktop binary name for this platform.
fn bin_name() -> &'static str {
    if cfg!(windows) {
        "darkrun-desktop.exe"
    } else {
        "darkrun-desktop"
    }
}

/// If `exe` lives in a cargo workspace's `target/<profile>/`, return
/// `(workspace_root, profile)` — the dev-checkout signal. Recognizes the darkrun
/// workspace by its `desktop/` crate. Pure over `exe` so it's testable.
fn dev_workspace_from(exe: &Path) -> Option<(PathBuf, String)> {
    let profile_dir = exe.parent()?; // <ws>/target/<profile>
    let profile = profile_dir.file_name()?.to_str()?.to_string();
    if profile != "debug" && profile != "release" {
        return None;
    }
    let target = profile_dir.parent()?; // <ws>/target
    if target.file_name()?.to_str()? != "target" {
        return None;
    }
    let ws = target.parent()?.to_path_buf(); // <ws>
    let is_darkrun_ws =
        ws.join("Cargo.toml").is_file() && ws.join("desktop").join("Cargo.toml").is_file();
    is_darkrun_ws.then_some((ws, profile))
}

/// The dev workspace + profile the running engine was built in, if any.
fn dev_workspace() -> Option<(PathBuf, String)> {
    dev_workspace_from(&std::env::current_exe().ok()?)
}

/// The outcome of [`spawn`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// The app was launched; carries the binary path.
    Launched(PathBuf),
    /// A dev build is in flight; the app will open when `cargo build` finishes.
    Building,
    /// No desktop binary could be resolved or built.
    NotFound,
}

/// Single-quote a path for a POSIX shell command.
#[cfg(not(windows))]
fn sh_quote(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"))
}

/// The synthesized DEV-bundle id: the identity the generated `.app` wrapper
/// carries (its `CFBundleIdentifier` in [`INFO_PLIST`] below must stay in
/// sync). Distinct from [`INSTALLED_BUNDLE_ID`] (the signed store app), so the
/// two never shadow each other in LaunchServices.
#[cfg(target_os = "macos")]
const DEV_BUNDLE_ID: &str = "ai.darkrun.desktop";

/// The minimal `Info.plist` for the macOS launch wrapper. `CFBundleName` is what
/// the Dock/menu-bar show ("darkrun"); the window title is set by the app itself.
#[cfg(target_os = "macos")]
const INFO_PLIST: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>darkrun-desktop</string>
  <key>CFBundleIdentifier</key><string>ai.darkrun.desktop</string>
  <key>CFBundleName</key><string>darkrun</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleShortVersionString</key><string>"#,
    env!("CARGO_PKG_VERSION"),
    r#"</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
"#
);

/// The app icon, embedded so the launch wrapper always ships its own `.icns`
/// (referenced by `CFBundleIconFile` above) without depending on a sibling file.
#[cfg(target_os = "macos")]
const APP_ICON: &[u8] = include_bytes!("../assets/AppIcon.icns");

/// The bundle's inner executable path for a given `.app`.
#[cfg(target_os = "macos")]
fn bundle_exe(bundle: &Path) -> PathBuf {
    bundle.join("Contents").join("MacOS").join("darkrun-desktop")
}

/// Materialize (idempotently) a tiny `.app` wrapper next to `bin` so `open` can
/// hand the launch to LaunchServices. Writes only the plist + icon + dirs (cheap,
/// and valid even before the binary is built); the executable itself is placed by
/// [`sync_bundle_exe`] / the build script. Returns the `.app` path.
///
/// The `Contents/MacOS` executable must be a **real copy** of the binary, NOT a
/// symlink: macOS resolves the main bundle from the executable's path, so a
/// symlink pointing at a binary *outside* the `.app` loses the bundle association
/// and the Dock falls back to a generic icon.
#[cfg(target_os = "macos")]
fn ensure_bundle(bin: &Path) -> std::io::Result<PathBuf> {
    let dir = bin.parent().unwrap_or_else(|| Path::new("."));
    let bundle = dir.join("darkrun-desktop.app");
    let macos = bundle.join("Contents").join("MacOS");
    std::fs::create_dir_all(&macos)?;
    std::fs::write(bundle.join("Contents").join("Info.plist"), INFO_PLIST)?;
    let resources = bundle.join("Contents").join("Resources");
    std::fs::create_dir_all(&resources)?;
    std::fs::write(resources.join("AppIcon.icns"), APP_ICON)?;
    Ok(bundle)
}

/// Copy `bin` into the bundle's `Contents/MacOS` (replacing any stale symlink or
/// older copy) so the launched app is a self-contained bundle with our icon. A
/// no-op when the copy is already current (same size + mtime), to avoid a 40 MB
/// copy on every launch. `touch`es the bundle so LaunchServices re-reads it.
#[cfg(target_os = "macos")]
fn sync_bundle_exe(bundle: &Path, bin: &Path) -> std::io::Result<()> {
    if !bin.is_file() {
        return Ok(());
    }
    let exe = bundle_exe(bundle);
    let current = std::fs::symlink_metadata(&exe);
    let needs_copy = match (&current, bin.metadata()) {
        // Re-copy if the dest is a symlink, a different size, or older than `bin`.
        (Ok(c), Ok(b)) => {
            c.file_type().is_symlink()
                || c.len() != b.len()
                || c.modified().ok() < b.modified().ok()
        }
        _ => true,
    };
    if needs_copy {
        let _ = std::fs::remove_file(&exe);
        std::fs::copy(bin, &exe)?;
        // Bump the bundle mtime so LaunchServices refreshes the cached icon.
        let _ = Command::new("touch").arg(bundle).status();
        // Ad-hoc re-sign the bundle so it carries a resource seal. The copied
        // binary has only its linker's Mach-O signature; without the bundle
        // seal, modern macOS refuses the launch (RBS error 5 / POSIX 153
        // "Launchd job spawn failed"), silently, into the launch log.
        let _ = Command::new("codesign")
            .args(["--force", "--deep", "-s", "-"])
            .arg(bundle)
            .status();
    }
    Ok(())
}

/// Spawn a **detached** `cargo build -p darkrun-desktop && <launch>` so the build
/// runs in the background and the app launches itself when it completes — the
/// `show` call doesn't block on the (one-time) compile. Build + app output go to
/// the launch log. Returns whether the builder process spawned.
fn spawn_build_then_launch(
    ws: &Path,
    profile: &str,
    bin: &Path,
    port: u16,
    repo_root: &Path,
    session: Option<&str>,
) -> bool {
    let rel = if profile == "release" { " --release" } else { "" };
    let (out, err) = log_stdio(repo_root);
    let mut cmd;
    #[cfg(target_os = "macos")]
    {
        // Pre-create the wrapper (symlink may dangle until the build lands), then
        // launch through LaunchServices so the app reaches the GUI session.
        let bundle = ensure_bundle(bin).map(|b| b.to_string_lossy().into_owned());
        let log = log_path(repo_root);
        // Pin to the run so the post-build launch opens straight to its Review.
        let sess = session
            .map(|s| format!(" --env DARKRUN_SESSION_ID={s}"))
            .unwrap_or_default();
        let script = match bundle {
            // After the build, copy the freshly-built binary INTO the bundle (a
            // real executable, not a symlink) so macOS keeps the bundle/icon
            // association, then `touch` the .app so LaunchServices re-reads it
            // (busting a stale icon cache), ad-hoc RE-SIGN the bundle, then
            // launch. The re-sign is load-bearing: the copied binary carries
            // only its linker's Mach-O signature with no bundle resource seal,
            // and modern macOS refuses to spawn such a bundle (RBS error 5 /
            // POSIX 153 "Launchd job spawn failed" — observed silently killing
            // every dev-desktop launch on a machine once it upgraded).
            Ok(bundle) => format!(
                "cargo build -p darkrun-desktop{rel} && rm -f {exe} && cp {bin} {exe} && touch {bnd} && \
                 codesign --force --deep -s - {bnd} && \
                 exec open -n {bnd} --env DARKRUN_PORT={port}{sess} --stdout {log} --stderr {log}",
                bin = sh_quote(bin),
                // `rm` first so we replace any stale symlink instead of copying
                // *through* it onto the source binary (which would leave the
                // symlink — and the broken icon — in place).
                exe = sh_quote(&bundle_exe(Path::new(&bundle))),
                bnd = sh_quote(Path::new(&bundle)),
                log = sh_quote(&log),
            ),
            // Bundle couldn't be written — fall back to a direct exec.
            Err(_) => format!(
                "cargo build -p darkrun-desktop{rel} && exec {}",
                sh_quote(bin)
            ),
        };
        cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let script = format!(
            "cargo build -p darkrun-desktop{rel} && exec {}",
            sh_quote(bin)
        );
        cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
    }
    #[cfg(windows)]
    {
        let script = format!(
            "cargo build -p darkrun-desktop{rel} && \"{}\"",
            bin.display()
        );
        cmd = Command::new("cmd");
        cmd.arg("/C").arg(script);
    }
    cmd.current_dir(ws)
        .env("DARKRUN_PORT", port.to_string());
    // Non-macOS launches inherit the builder's env (macOS uses `open --env` above).
    match session {
        Some(s) => {
            cmd.env("DARKRUN_SESSION_ID", s);
        }
        None => {
            cmd.env_remove("DARKRUN_SESSION_ID");
        }
    }
    cmd.stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .is_ok()
}

/// Locate the `darkrun-desktop` binary WITHOUT building — an explicit
/// `DARKRUN_DESKTOP` path, a sibling of the running engine binary (installed
/// plugin), then the project's `target/{release,debug}`. `None` when not found.
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
    for prof in ["release", "debug"] {
        let p = repo_root.join("target").join(prof).join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Launch a resolved binary pointed at the engine `port`, unpinned
/// (`DARKRUN_SESSION_ID` cleared) so it opens the run-browser home, whose
/// `current`-focus poller navigates to the run the engine just raised. Output is
/// captured to the launch log.
///
/// On **macOS** this goes through LaunchServices (`open` on a generated `.app`
/// wrapper) so the app reaches the login GUI session even though the MCP server
/// is spawned outside it — a direct `exec` there is killed by AppKit before a
/// window appears. Elsewhere a direct detached spawn is fine.
#[cfg(target_os = "macos")]
fn launch(bin: PathBuf, port: u16, repo_root: &Path, session: Option<&str>) -> Launch {
    let bundle = match ensure_bundle(&bin) {
        Ok(b) => b,
        Err(_) => return launch_direct(bin, port, repo_root, session),
    };
    // Put a real copy of the binary inside the bundle (replacing any stale
    // symlink) so the Dock shows our icon; a no-op when already current. This
    // also carries the ad-hoc re-seal (see sync_bundle_exe), which stays in
    // place for the next cold start on both paths below.
    let _ = sync_bundle_exe(&bundle, &bin);
    // WARM handoff, mirroring launch_installed: with a run to show and the dev
    // bundle already running, hand it the review deep link (routed to the live
    // instance as a GetURL apple event, tao `Event::Opened`) so it FOLLOWS the
    // run. `open -n` here would instead spawn a second window still pinned to a
    // stale env port; a warm app never re-reads `--env`. The COLD case keeps
    // the `-n`-by-path launch below: the synthesized id can be registered to
    // several checkouts, so only the path form is guaranteed to open THIS build.
    if let Some(s) = session {
        if app_running(DEV_BUNDLE_ID) && open_deeplink(DEV_BUNDLE_ID, s) {
            return Launch::Launched(bin);
        }
    }
    let log = log_path(repo_root);
    let _ = open_log(repo_root); // ensure .darkrun/ exists for open's redirect
    // `open` blocks only until LaunchServices accepts the launch, so a non-zero
    // status is a real "couldn't start" signal — unlike a bare fork succeeding.
    let mut cmd = Command::new("open");
    cmd.arg("-n")
        .arg(&bundle)
        .arg("--env")
        .arg(format!("DARKRUN_PORT={port}"));
    // Pin to the run so the app opens straight to its Review (`open` launches in
    // a clean launchd env, so DARKRUN_SESSION_ID must be passed explicitly).
    if let Some(s) = session {
        cmd.arg("--env").arg(format!("DARKRUN_SESSION_ID={s}"));
    }
    let ok = cmd
        .arg("--stdout")
        .arg(&log)
        .arg("--stderr")
        .arg(&log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        Launch::Launched(bin)
    } else {
        Launch::NotFound
    }
}

#[cfg(not(target_os = "macos"))]
fn launch(bin: PathBuf, port: u16, repo_root: &Path, session: Option<&str>) -> Launch {
    launch_direct(bin, port, repo_root, session)
}

/// Direct detached spawn (non-macOS, or the macOS bundle fallback). Output goes
/// to the launch log so a crash is traceable. With `session` set the app is
/// PINNED to that run (`DARKRUN_SESSION_ID`) so it opens straight to the Review;
/// `None` opens the unpinned projects home.
fn launch_direct(bin: PathBuf, port: u16, repo_root: &Path, session: Option<&str>) -> Launch {
    let (out, err) = log_stdio(repo_root);
    let mut cmd = Command::new(&bin);
    cmd.env("DARKRUN_PORT", port.to_string());
    match session {
        Some(s) => {
            cmd.env("DARKRUN_SESSION_ID", s);
        }
        None => {
            cmd.env_remove("DARKRUN_SESSION_ID");
        }
    }
    let ok = cmd
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .is_ok();
    if ok {
        Launch::Launched(bin)
    } else {
        Launch::NotFound
    }
}

/// The single darkrun app bundle id (Mac App Store / TestFlight build), shared
/// across platforms — see `desktop/Dioxus.toml`. Used to open the INSTALLED,
/// signed app by identity rather than by path.
#[cfg(target_os = "macos")]
const INSTALLED_BUNDLE_ID: &str = "ai.darkrun.app";

/// The custom-scheme deep link that points the app at a run's live Review:
/// `darkrun://review/<slug>`.
///
/// Passed to `open` as the URL argument (see [`launch_installed`]) so an
/// ALREADY-RUNNING app receives it through tao `Event::Opened` (parsed by
/// `wire::parse_review_deeplink`) and navigates to the live run. The env-only
/// relaunch (`--env DARKRUN_PORT=…`) is IGNORED by a warm app — macOS never
/// re-injects `--env` into a running process — which is why an app pinned at a
/// since-recycled ephemeral port kept subscribing to a dead port and never
/// registered presence. The deep link is what makes a warm app FOLLOW the run,
/// while `--env` still seeds a COLD launch.
///
/// The slug is a kebab-case run slug (`[a-z0-9-]`), safe verbatim in a URL path.
/// Pure over the slug, so the built URL is unit-tested.
#[cfg(target_os = "macos")]
fn review_deeplink(session: &str) -> String {
    format!("darkrun://review/{session}")
}

/// Whether a GUI app with `bundle_id` is currently RUNNING in the login
/// session, per LaunchServices (`lsappinfo` prints nothing for an app that
/// isn't up). Best-effort: a probe failure reads as "not running", degrading
/// to the cold-launch path (a redundant window at worst, never a lost gate).
#[cfg(target_os = "macos")]
fn app_running(bundle_id: &str) -> bool {
    Command::new("lsappinfo")
        .args(["info", "-app", bundle_id])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| o.status.success() && !o.stdout.trim_ascii().is_empty())
        .unwrap_or(false)
}

/// Hand a run's review deep link to an ALREADY-RUNNING app by bundle id:
/// `open -b <id> darkrun://review/<slug>` routes the URL to the live instance
/// (GetURL apple event, tao `Event::Opened`) so it navigates to the run
/// without a second window spawning. Returns whether `open` accepted it; a
/// refusal falls back to the caller's cold-launch path.
#[cfg(target_os = "macos")]
fn open_deeplink(bundle_id: &str, session: &str) -> bool {
    Command::new("open")
        .arg("-b")
        .arg(bundle_id)
        .arg(review_deeplink(session))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Open the **installed** app (Mac App Store / TestFlight) by bundle id via
/// LaunchServices, pointed at the engine `port`.
///
/// Unlike [`launch`], this never rebuilds, wraps, or copies a binary — it opens
/// the already-installed, signed `.app` as-is (`open -b`), which is the only way
/// to drive the sandboxed store build (re-wrapping its signed binary would fail).
/// The store app carries `network.client`, so it can reach `ws://127.0.0.1:port`.
/// Requested via `DARKRUN_DESKTOP=installed`, so a dev-checkout engine (which
/// otherwise always rebuilds + launches the local `target/` binary) can drive the
/// SHIPPED app instead.
#[cfg(target_os = "macos")]
fn launch_installed(port: u16, repo_root: &Path, session: Option<&str>) -> Launch {
    let log = log_path(repo_root);
    let _ = open_log(repo_root); // ensure .darkrun/ exists for open's redirect
    let mut cmd = Command::new("open");
    cmd.arg("-b").arg(INSTALLED_BUNDLE_ID);
    // Open a NEW instance only when there's no run to deep-link to. With a run,
    // we pass its deep link as the `open` URL argument (below): macOS routes it
    // to an ALREADY-RUNNING instance via the GetURL apple event (tao
    // `Event::Opened`), so the warm app FOLLOWS the live engine. `-n` would
    // instead spawn a redundant second window still stuck on a stale env port,
    // which is exactly the surfacing failure we're fixing.
    if session.is_none() {
        cmd.arg("-n");
    }
    cmd.arg("--env").arg(format!("DARKRUN_PORT={port}"));
    if let Some(s) = session {
        // COLD launch reads these from the fresh process env; a warm app ignores
        // them (macOS doesn't re-inject `--env`) and navigates via the deep link.
        cmd.arg("--env").arg(format!("DARKRUN_SESSION_ID={s}"));
    }
    cmd.arg("--stdout").arg(&log).arg("--stderr").arg(&log);
    // The run's deep link, LAST so `open` reads it as the URL to open with the
    // bundle. Delivered to a running instance as `Event::Opened`; on a cold
    // launch tao queues it and replays it once the event loop starts.
    if let Some(s) = session {
        cmd.arg(review_deeplink(s));
    }
    // `open -b` exits non-zero when the bundle id isn't installed, so a failure
    // here is a real "not installed" signal — surface it as NotFound.
    let ok = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        Launch::Launched(PathBuf::from(INSTALLED_BUNDLE_ID))
    } else {
        Launch::NotFound
    }
}

/// Whether the shipped "Darkrun AI" app is installed. The Mac App Store /
/// TestFlight install it to `/Applications` (or `~/Applications`), so a cheap
/// path probe suffices — no Spotlight dependency, no subprocess.
#[cfg(target_os = "macos")]
fn installed_app_present() -> bool {
    const APP: &str = "Darkrun AI.app";
    if Path::new("/Applications").join(APP).is_dir() {
        return true;
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Path::new(&home).join("Applications").join(APP).is_dir();
    }
    false
}

/// Bring up the desktop app pointed at the engine `port`.
///
/// Resolution order:
/// - `DARKRUN_DESKTOP=installed` (macOS): force the shipped store / TestFlight app.
/// - `DARKRUN_DESKTOP=<path>`: launch that binary.
/// - **Dev checkout, macOS**: if the shipped "Darkrun AI" app is installed it is
///   AUTO-DETECTED and driven (so a dev-checkout engine smoke-tests the released
///   app without rebuilding the local desktop). `DARKRUN_DESKTOP=local` forces the
///   local `target/<profile>/darkrun-desktop` build instead, for desktop-UI work.
/// - **Dev checkout otherwise**: the local build, compiled on demand.
/// - Otherwise: the installed sibling binary.
pub fn spawn(repo_root: &Path, port: u16, session: Option<&str>) -> Launch {
    let want = std::env::var("DARKRUN_DESKTOP").ok();
    // `DARKRUN_DESKTOP=installed`: force the SHIPPED, signed store app by bundle
    // id. Checked before the path form so the sentinel isn't read as a file path.
    #[cfg(target_os = "macos")]
    if want.as_deref() == Some("installed") {
        return launch_installed(port, repo_root, session);
    }
    // Explicit binary path (ignoring the `installed` / `local` sentinels).
    if let Some(path) = want.as_deref().filter(|w| *w != "installed" && *w != "local") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return launch(p, port, repo_root, session);
        }
    }
    if let Some((ws, profile)) = dev_workspace() {
        // Dev checkout: prefer the AUTO-DETECTED installed app so this engine can
        // drive the SHIPPED build without rebuilding the local desktop each time.
        // `DARKRUN_DESKTOP=local` opts back into the local build for desktop-UI
        // development (where you want to see your own changes).
        #[cfg(target_os = "macos")]
        if want.as_deref() != Some("local") && installed_app_present() {
            return launch_installed(port, repo_root, session);
        }
        // Otherwise build the local desktop on demand and launch it (ALWAYS
        // rebuild first so the app reflects the latest source; `cargo build` is a
        // near-instant no-op when nothing changed). Fall back to launching
        // whatever exists if the builder can't even be spawned.
        let bin = ws.join("target").join(&profile).join(bin_name());
        if spawn_build_then_launch(&ws, &profile, &bin, port, repo_root, session) {
            return Launch::Building;
        }
        if bin.is_file() {
            return launch(bin, port, repo_root, session);
        }
    }
    // Installed plugin: sibling of the engine binary, or the project target dir.
    match find(repo_root) {
        Some(bin) => launch(bin, port, repo_root, session),
        None => Launch::NotFound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_mode_awaits_when_a_client_is_present() {
        // Live and Lost(in-grace) both read as present → never relaunch.
        assert_eq!(surface_mode(Presence::Live, false), SurfaceMode::Await);
        assert_eq!(surface_mode(Presence::Live, true), SurfaceMode::Await);
        assert_eq!(surface_mode(Presence::Lost, false), SurfaceMode::Await);
        assert_eq!(surface_mode(Presence::Lost, true), SurfaceMode::Await);
    }

    #[test]
    fn surface_mode_awaits_when_not_present_but_a_device_acked() {
        // No live client, but a device confirmed the push receipt → hold.
        assert_eq!(surface_mode(Presence::Closed, true), SurfaceMode::Await);
        assert_eq!(
            surface_mode(Presence::NeverAttached, true),
            SurfaceMode::Await
        );
    }

    #[test]
    fn surface_mode_launches_when_not_present_and_not_acked() {
        // No live client and no acked device → open the app (historical behavior).
        assert_eq!(surface_mode(Presence::Closed, false), SurfaceMode::Launch);
        assert_eq!(
            surface_mode(Presence::NeverAttached, false),
            SurfaceMode::Launch
        );
    }

    fn touch(path: &Path) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn dev_workspace_detects_a_cargo_target_layout() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        // A darkrun-shaped workspace.
        touch(&ws.join("Cargo.toml"));
        touch(&ws.join("desktop").join("Cargo.toml"));
        let exe = ws.join("target").join("debug").join("darkrun");
        touch(&exe);

        let (got_ws, profile) = dev_workspace_from(&exe).expect("dev workspace");
        assert_eq!(got_ws, ws);
        assert_eq!(profile, "debug");
    }

    #[test]
    fn dev_workspace_rejects_non_target_and_non_darkrun_layouts() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        // Not under target/.
        let stray = ws.join("bin").join("darkrun");
        touch(&stray);
        assert!(dev_workspace_from(&stray).is_none());
        // Under target/ but not the darkrun workspace (no desktop/ crate).
        let exe = ws.join("target").join("release").join("darkrun");
        touch(&exe);
        touch(&ws.join("Cargo.toml"));
        assert!(dev_workspace_from(&exe).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn review_deeplink_is_the_custom_scheme_review_url() {
        // The URL a warm-app launch hands to `open` — the custom-scheme review
        // link the running app parses (`wire::parse_review_deeplink`) to follow
        // the live run. A kebab-case slug passes through verbatim.
        assert_eq!(
            review_deeplink("quiet-tumbling-canyon"),
            "darkrun://review/quiet-tumbling-canyon"
        );
        assert_eq!(review_deeplink("r"), "darkrun://review/r");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_running_is_false_for_a_bundle_id_that_is_not_up() {
        // The warm-handoff guard: an id nothing has ever registered can't be
        // running, so the probe must say "not running" (and the caller then
        // keeps the cold `open -n` path). Also covers the headless-CI case,
        // where lsappinfo itself errors: that too must read as not running.
        assert!(!app_running("ai.darkrun.test-never-installed"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ensure_bundle_writes_plist_and_icon() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("darkrun-desktop");
        touch(&bin);
        let bundle = ensure_bundle(&bin).expect("bundle");

        let plist = std::fs::read_to_string(bundle.join("Contents").join("Info.plist")).unwrap();
        assert!(plist.contains("<key>CFBundleIconFile</key><string>AppIcon</string>"));

        let icon = bundle
            .join("Contents")
            .join("Resources")
            .join("AppIcon.icns");
        let bytes = std::fs::read(&icon).expect("icon written");
        assert_eq!(bytes, APP_ICON);
        // .icns files begin with the "icns" magic.
        assert_eq!(&bytes[..4], b"icns");
    }

    // DARKRUN_DESKTOP is process-global; keep its mutation in one sequential test.
    #[test]
    fn find_resolves_env_then_target() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join(bin_name());
        touch(&fake);
        std::env::set_var("DARKRUN_DESKTOP", &fake);
        assert_eq!(find(dir.path()).as_deref(), Some(fake.as_path()));

        std::env::remove_var("DARKRUN_DESKTOP");
        let repo = tempfile::tempdir().unwrap();
        let bin = repo.path().join("target").join("release").join(bin_name());
        touch(&bin);
        assert_eq!(find(repo.path()), Some(bin));
    }
}
