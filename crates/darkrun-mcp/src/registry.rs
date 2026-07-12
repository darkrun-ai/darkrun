//! Home discovery registry — `~/.darkrun/<slug>/engine-<pid>.json`.
//!
//! When a `darkrun mcp` engine boots it binds an EPHEMERAL loopback port (so
//! many engines coexist without colliding on a fixed port) and then advertises
//! itself by writing a small JSON [`EngineDescriptor`] under the user's home:
//!
//! ```text
//! ~/.darkrun/<slug>/engine-<pid>.json
//! ```
//!
//! The `<slug>` is derived from the engine's repo root so all engines for one
//! repo share a directory; the `<pid>` keeps concurrent engines for the SAME
//! repo from clobbering each other's descriptor. A discoverer (the desktop app)
//! scans this tree to find LIVE engines and the port each is listening on — no
//! fixed port, no environment handshake.
//!
//! Descriptors are RETAINED on exit, never deleted: a clean shutdown flags the
//! file stale (renames it to `engine-<pid>.json.stale`) and [`list_live_engines`]
//! additionally treats any descriptor whose pid is no longer running as stale.
//! Keeping the on-disk record (rather than deleting it) leaves a discoverable
//! trail for debugging and tolerates engines that die without running their
//! shutdown hook.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use darkrun_core::domain::ProjectRecord;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// One engine's discovery descriptor: the LIVE record a discoverer reads to find
/// a running `darkrun mcp` engine and the loopback port it serves on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EngineDescriptor {
    /// OS process id of the engine. Used to check liveness (signal 0) and to
    /// name the descriptor file so concurrent engines for one repo don't clash.
    pub pid: u32,
    /// The loopback address the engine's HTTP/WS server is listening on, with
    /// the ACTUAL (post-bind) port — never `0`.
    pub addr: SocketAddr,
    /// Absolute repo root the engine was launched against.
    pub repo_root: PathBuf,
    /// Slug derived from `repo_root`; matches the parent directory name.
    pub slug: String,
    /// Harness key the engine adapted to (e.g. the agent flavor), for display.
    pub harness: String,
    /// RFC3339 timestamp the descriptor was written at boot.
    pub started_at: String,
    /// How a client can reach this engine: always a LOCAL candidate (the loopback
    /// `addr`, connectable without auth from the same machine), plus a RELAY
    /// candidate when the engine is configured for remote access (the host has
    /// dialed the relay). A local-only engine carries just the local candidate.
    #[serde(default)]
    pub reachability: darkrun_api::tunnel::Reachability,
}

/// The registry rooted at `~/.darkrun`, owning the descriptor lifecycle for ONE
/// engine: derive the slug, write the boot descriptor, and (on shutdown) flag it
/// stale.
#[derive(Debug, Clone)]
pub struct EngineRegistry {
    /// Root of the discovery tree (`~/.darkrun`).
    root: PathBuf,
    /// Absolute repo root this engine serves; recorded in the descriptor.
    repo_root: PathBuf,
    /// Slug for this engine's repo (the `<slug>` directory name).
    slug: String,
    /// This engine's pid.
    pid: u32,
    /// The relay candidate to advertise, when remote access is enabled. `None`
    /// for a local-only engine (the descriptor then carries only the loopback
    /// local candidate).
    relay: Option<darkrun_api::tunnel::RelayCandidate>,
}

impl EngineRegistry {
    /// Build a registry for `repo_root` rooted at the default `~/.darkrun`,
    /// deriving the slug and capturing the current pid.
    ///
    /// Fails only when the home directory can't be resolved.
    #[cfg(not(tarpaulin_include))] // resolves the real home dir; logic via with_root
    pub fn new(repo_root: impl AsRef<Path>) -> io::Result<Self> {
        let root = default_root().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "could not resolve home directory for the darkrun discovery registry",
            )
        })?;
        Ok(Self::with_root(root, repo_root))
    }

    /// Like [`new`](Self::new) but with an explicit registry `root`. Used by
    /// tests to point the tree at a temp dir.
    pub fn with_root(root: impl Into<PathBuf>, repo_root: impl AsRef<Path>) -> Self {
        let repo_root = repo_root.as_ref().to_path_buf();
        Self {
            root: root.into(),
            slug: slug_for(&repo_root),
            repo_root,
            pid: std::process::id(),
            relay: None,
        }
    }

    /// Advertise a RELAY candidate alongside the local one — set when the engine
    /// has remote access enabled (it dials the relay). A discoverer then learns
    /// both how to reach the engine locally and how to tunnel in remotely.
    pub fn with_relay(mut self, relay: darkrun_api::tunnel::RelayCandidate) -> Self {
        self.relay = Some(relay);
        self
    }

    /// The slug directory for this engine's repo (`<root>/<slug>`).
    pub fn slug_dir(&self) -> PathBuf {
        self.root.join(&self.slug)
    }

    /// The live descriptor path for this engine (`<slug_dir>/engine-<pid>.json`).
    pub fn descriptor_path(&self) -> PathBuf {
        self.slug_dir().join(format!("engine-{}.json", self.pid))
    }

    /// Write the boot descriptor advertising `addr` (the ACTUAL bound port) and
    /// `harness`, creating the slug directory if needed.
    ///
    /// Returns the descriptor written. Best-effort: callers treat a failure as
    /// non-fatal (the engine still serves; it just isn't auto-discoverable).
    pub fn announce(&self, addr: SocketAddr, harness: &str) -> io::Result<EngineDescriptor> {
        fs::create_dir_all(self.slug_dir())?;
        let reachability = darkrun_api::tunnel::Reachability {
            local: Some(darkrun_api::tunnel::LocalCandidate {
                host: addr.ip().to_string(),
                port: addr.port(),
            }),
            relay: self.relay.clone(),
        };
        let descriptor = EngineDescriptor {
            pid: self.pid,
            addr,
            repo_root: self.repo_root.clone(),
            slug: self.slug.clone(),
            harness: harness.to_string(),
            started_at: now_rfc3339(),
            reachability,
        };
        let json = serde_json::to_vec_pretty(&descriptor)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(self.descriptor_path(), json)?;
        Ok(descriptor)
    }

    /// Flag this engine's descriptor stale on exit by renaming it to
    /// `engine-<pid>.json.stale`. RETAINS the record (never deletes it).
    ///
    /// Idempotent and best-effort: a missing descriptor (already flagged, or
    /// never written) is a no-op.
    pub fn mark_stale(&self) -> io::Result<()> {
        let live = self.descriptor_path();
        if !live.exists() {
            return Ok(());
        }
        let stale = stale_path(&live);
        fs::rename(&live, &stale)
    }
}

/// The app-group identifier shared by the engine CLI and the Mac App Store
/// desktop app — the macOS data home (see [`data_home`]) is its container.
#[cfg(target_os = "macos")]
pub const APP_GROUP: &str = "group.ai.darkrun";

/// The base directory under which darkrun keeps its cross-process state: the
/// engine-discovery registry ([`default_root`]) and cloned projects
/// ([`default_clone_root`]).
///
/// On **macOS** this is the app-group container
/// `~/Library/Group Containers/group.ai.darkrun/`, the one place both the
/// UNSANDBOXED engine CLI and the SANDBOXED Mac App Store desktop app can meet:
/// any process may write that path, but only a sandboxed app needs the
/// `com.apple.security.application-groups` entitlement to reach it. Sharing it
/// means the App Store app discovers engines the CLI started (and operates on the
/// same cloned repos). Everywhere else it's the home directory, preserving the
/// historical `~/.darkrun` + `~/darkrun` layout exactly.
fn data_home() -> Option<PathBuf> {
    let home = dirs::home_dir().or_else(home_dir_env)?;
    #[cfg(target_os = "macos")]
    {
        Some(home.join("Library").join("Group Containers").join(APP_GROUP))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some(home)
    }
}

/// Resolve the default discovery root (`~/.darkrun`, or the app-group container's
/// `.darkrun` on macOS — see [`data_home`]).
pub fn default_root() -> Option<PathBuf> {
    data_home().map(|base| base.join(".darkrun"))
}

/// Resolve the default root for CLONED projects (`~/darkrun`, or the app-group
/// container's `darkrun` on macOS). Lives beside the discovery registry so the
/// sandboxed Mac App Store app can read/write the repos the engine operates on.
pub fn default_clone_root() -> Option<PathBuf> {
    data_home().map(|base| base.join("darkrun"))
}

/// Env-var fallback mirroring the resolution used elsewhere in darkrun.
#[cfg(not(tarpaulin_include))] // env-var home fallback; only on dirs::home_dir() failure
fn home_dir_env() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// Derive the `<slug>` directory name for `repo_root`.
///
/// The slug is the sanitized basename of the path; to keep slugs unique across
/// different repos that share a basename (e.g. two `app` checkouts), a short
/// hash of the FULL path is appended. The result is a filesystem-safe,
/// collision-resistant directory name.
pub fn slug_for(repo_root: &Path) -> String {
    // A project IS its git repository, not whatever directory the engine
    // happened to boot in: a linked WORKTREE resolves to the main checkout
    // (so every worktree of a repo groups under ONE project), and the display
    // base prefers the origin remote's repo name over a local dir name.
    let canonical = resolve_project_root(repo_root);
    let base = origin_repo_name(&canonical)
        .or_else(|| {
            canonical
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "root".to_string());
    let sanitized = sanitize(&base);
    let hash = short_hash(&canonical);
    format!("{sanitized}-{hash}")
}

/// Resolve a checkout dir to its project root (worktree → main checkout).
fn resolve_project_root(repo_root: &Path) -> PathBuf {
    darkrun_git::project_root_of(repo_root)
}

/// The repository name from the `origin` remote (`acme/store.git` → `store`),
/// when the project has one.
fn origin_repo_name(root: &Path) -> Option<String> {
    use darkrun_git::GitBackend;
    let url = darkrun_git::Git::open(root).ok()?.remote_url("origin").ok()??;
    let trimmed = url.trim().trim_end_matches('/');
    let stem = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let name = stem.rsplit(['/', ':']).next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Replace any character that isn't `[A-Za-z0-9._-]` with `-`, collapsing the
/// result so it's safe as a single path component.
fn sanitize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    // Avoid leading dots so the slug dir isn't accidentally hidden, and trim
    // dashes for a tidy name.
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "repo".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Short hex digest of the full repo path, for slug uniqueness.
fn short_hash(repo_root: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(repo_root.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    // First 4 bytes as hex (8 chars) is plenty to disambiguate basenames.
    let mut s = String::with_capacity(8);
    for byte in &digest[..4] {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

/// The `.stale` sibling path for a live descriptor.
fn stale_path(live: &Path) -> PathBuf {
    let mut name = live
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(".stale");
    live.with_file_name(name)
}

/// Read every LIVE engine descriptor under the default `~/.darkrun` tree.
///
/// A descriptor is live when its file is the active `engine-<pid>.json` (not a
/// `.stale` sibling) AND its pid is still running. Stale-but-running is ignored
/// (already flagged); live-but-dead is ignored (engine vanished without a clean
/// shutdown). Returns an empty list when the tree doesn't exist.
#[cfg(not(tarpaulin_include))] // resolves the real home dir; logic via list_live_engines_in
pub fn list_live_engines() -> io::Result<Vec<EngineDescriptor>> {
    match default_root() {
        Some(root) => list_live_engines_in(&root),
        None => Ok(Vec::new()),
    }
}

/// Like [`list_live_engines`] but scans an explicit `root`. Used by tests.
pub fn list_live_engines_in(root: &Path) -> io::Result<Vec<EngineDescriptor>> {
    let mut live = Vec::new();
    let slug_dirs = match fs::read_dir(root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(live),
        Err(e) => return Err(e),
    };
    for slug_entry in slug_dirs.flatten() {
        let slug_path = slug_entry.path();
        if !slug_path.is_dir() {
            continue;
        }
        let descriptors = match fs::read_dir(&slug_path) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in descriptors.flatten() {
            let path = entry.path();
            if !is_live_descriptor_name(&path) {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else { continue };
            let Ok(descriptor) = serde_json::from_slice::<EngineDescriptor>(&bytes) else {
                continue;
            };
            if process_alive(descriptor.pid) {
                live.push(descriptor);
            }
        }
    }
    Ok(live)
}

/// True when `path` is an active `engine-*.json` descriptor (not a `.stale`
/// sibling and not some other file).
fn is_live_descriptor_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    name.starts_with("engine-") && name.ends_with(".json")
}

/// Check whether process `pid` is currently running.
///
/// On Unix this sends signal 0 (the no-op liveness probe): `Ok` or
/// `EPERM` mean the process exists; `ESRCH` means it's gone. On other platforms
/// we can't cheaply probe, so we conservatively report `true` (the caller falls
/// back to treating descriptors as live until a clean shutdown flags them).
#[cfg(unix)]
pub fn process_alive(pid: u32) -> bool {
    use nix::errno::Errno;
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    matches!(
        kill(Pid::from_raw(pid as i32), None),
        Ok(()) | Err(Errno::EPERM)
    )
}

/// See the Unix implementation. On non-Unix targets we can't cheaply probe, so
/// conservatively report `true`.
#[cfg(not(unix))]
pub fn process_alive(_pid: u32) -> bool {
    true
}

/// RFC3339 timestamp for `started_at`.
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// The project-record filename written inside a slug directory.
const PROJECT_RECORD_FILE: &str = "project.json";

/// Register `repo_root` as a project under the default `~/.darkrun` tree.
///
/// Derives the slug from `repo_root` (the SAME [`slug_for`] logic the engine
/// uses, so a later engine boot lands in the same `<slug>` directory), then
/// writes a durable [`ProjectRecord`] to `~/.darkrun/<slug>/project.json`. The
/// record persists independently of any live engine, so the desktop can list
/// registered-but-idle projects.
///
/// `name` is an optional display label; `added_at` is stamped now. Returns the
/// record that was written. Fails when the home directory can't be resolved.
#[cfg(not(tarpaulin_include))] // resolves the real home dir; logic via register_project_in
pub fn register_project(repo_root: &Path, name: Option<String>) -> io::Result<ProjectRecord> {
    let root = default_root().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not resolve home directory for the darkrun discovery registry",
        )
    })?;
    register_project_in(&root, repo_root, name)
}

/// Like [`register_project`] but under an explicit registry `root`. Used by
/// tests to point the tree at a temp dir.
pub fn register_project_in(
    root: &Path,
    repo_root: &Path,
    name: Option<String>,
) -> io::Result<ProjectRecord> {
    // A project IS its git repository: register the CANONICAL root (a linked
    // worktree resolves to the main checkout) under the canonical slug, and
    // default the display name to the repository's name — never a worktree
    // directory's.
    let canonical = resolve_project_root(repo_root);
    let slug = slug_for(&canonical);
    let record = ProjectRecord {
        slug: slug.clone(),
        name: name.or_else(|| display_name_for(&canonical)),
        path: canonical,
        added_at: Some(now_rfc3339()),
    };
    write_project_record_in(root, &slug, &record)?;
    Ok(record)
}

/// The human display name for a project root: the origin remote's repo name,
/// else the directory basename.
fn display_name_for(canonical: &Path) -> Option<String> {
    origin_repo_name(canonical).or_else(|| {
        canonical
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
    })
}

/// Write `record` to `<root>/<slug>/project.json`, creating the slug directory
/// if needed. Overwrites any existing record (re-registering is idempotent).
pub fn write_project_record_in(
    root: &Path,
    slug: &str,
    record: &ProjectRecord,
) -> io::Result<()> {
    let dir = root.join(slug);
    fs::create_dir_all(&dir)?;
    let json = serde_json::to_vec_pretty(record)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(dir.join(PROJECT_RECORD_FILE), json)
}

/// Read the [`ProjectRecord`] for `slug` from `<root>/<slug>/project.json`.
///
/// Returns `Ok(None)` when no record exists for that slug (the directory may
/// hold only engine descriptors, or not exist at all). A malformed record is a
/// hard error so callers can surface a corrupt registry; bulk scans use
/// [`list_projects_in`] which skips malformed entries instead.
pub fn read_project_record_in(root: &Path, slug: &str) -> io::Result<Option<ProjectRecord>> {
    let path = root.join(slug).join(PROJECT_RECORD_FILE);
    match fs::read(&path) {
        Ok(bytes) => {
            let record = serde_json::from_slice::<ProjectRecord>(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Some(record))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// List every registered project under the default `~/.darkrun` tree.
///
/// Scans each `<slug>/project.json` and returns the deserialized records — the
/// durable counterpart to [`list_live_engines`] (which surfaces only LIVE
/// engines). Idle projects (a `project.json` with no running engine) appear
/// here; the desktop overlays live status by matching on slug. Returns an empty
/// list when the tree doesn't exist.
#[cfg(not(tarpaulin_include))] // resolves the real home dir; logic via list_projects_in
pub fn list_projects() -> io::Result<Vec<ProjectRecord>> {
    match default_root() {
        Some(root) => list_projects_in(&root),
        None => Ok(Vec::new()),
    }
}

/// Like [`list_projects`] but scans an explicit `root`. Used by tests.
///
/// Robust to a partly-populated tree: a slug dir without a `project.json`, or
/// one whose record is malformed, is skipped rather than failing the whole scan
/// (legacy engine-only directories pre-date project registration).
pub fn list_projects_in(root: &Path) -> io::Result<Vec<ProjectRecord>> {
    let mut projects = Vec::new();
    let slug_dirs = match fs::read_dir(root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(projects),
        Err(e) => return Err(e),
    };
    for slug_entry in slug_dirs.flatten() {
        let slug_path = slug_entry.path();
        if !slug_path.is_dir() {
            continue;
        }
        let record_path = slug_path.join(PROJECT_RECORD_FILE);
        let Ok(bytes) = fs::read(&record_path) else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<ProjectRecord>(&bytes) else {
            continue;
        };
        // PRUNE dead projects: a record whose repo path no longer exists on
        // disk (a deleted clone, a /tmp demo, a retired worktree) is gone for
        // good — retire its dir rather than surfacing a ghost in the sidebar.
        if !record.path.exists() {
            let _ = fs::remove_dir_all(&slug_path);
            continue;
        }
        // SELF-HEAL stale identities: records written before project identity
        // was canonicalized may be keyed by a worktree directory (its name as
        // the slug, its path as the root). Re-derive the canonical identity;
        // when it differs, migrate the record (and any engine descriptors)
        // under the canonical slug and retire the stale dir — so a worktree
        // never surfaces as its own project.
        let canonical = resolve_project_root(&record.path);
        let expected = slug_for(&canonical);
        let dir_name = slug_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if dir_name != expected || record.path != canonical {
            let healed = ProjectRecord {
                slug: expected.clone(),
                name: record
                    .name
                    .clone()
                    .filter(|n| !dir_name.starts_with(n.as_str()))
                    .or_else(|| display_name_for(&canonical)),
                path: canonical,
                added_at: record.added_at.clone(),
            };
            // Write the canonical record only when one doesn't already exist
            // (an existing canonical registration wins).
            let dest_dir = root.join(&expected);
            let dest_record = dest_dir.join(PROJECT_RECORD_FILE);
            let _ = fs::create_dir_all(&dest_dir);
            if !dest_record.exists() {
                if let Ok(json) = serde_json::to_vec_pretty(&healed) {
                    let _ = fs::write(&dest_record, json);
                }
            }
            // Carry engine descriptors across, then retire the stale dir.
            if let Ok(rd) = fs::read_dir(&slug_path) {
                for f in rd.flatten() {
                    let from = f.path();
                    if from.file_name() != Some(PROJECT_RECORD_FILE.as_ref()) {
                        if let Some(fname) = from.file_name() {
                            let _ = fs::rename(&from, dest_dir.join(fname));
                        }
                    }
                }
            }
            let _ = fs::remove_file(&record_path);
            let _ = fs::remove_dir(&slug_path);
            // Surface the healed record unless the canonical dir already
            // carried its own (avoid duplicates in one pass).
            if !projects.iter().any(|p: &ProjectRecord| p.slug == expected) {
                if let Ok(b) = fs::read(&dest_record) {
                    if let Ok(rec) = serde_json::from_slice::<ProjectRecord>(&b) {
                        projects.push(rec);
                        continue;
                    }
                }
                projects.push(healed);
            }
            continue;
        }
        if !projects.iter().any(|p: &ProjectRecord| p.slug == record.slug) {
            projects.push(record);
        }
    }
    Ok(projects)
}

/// The newest engine descriptor in a slug directory, LIVE or historical.
///
/// [`list_live_engines_in`] deliberately ignores dead pids and `.stale` records;
/// this does the opposite, for BACKFILL: it reads every `engine-*.json` (and its
/// retired `engine-*.json.stale` sibling), alive or long gone, and returns the
/// one written most recently (by `started_at`). It is how a project that has RUN
/// but was never explicitly registered is recovered, since the descriptor carries
/// the `repo_root` the session ran against.
fn newest_engine_descriptor_in(slug_path: &Path) -> Option<EngineDescriptor> {
    let entries = fs::read_dir(slug_path).ok()?;
    let mut newest: Option<EngineDescriptor> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        // Any engine record, active (`engine-<pid>.json`) or retired
        // (`engine-<pid>.json.stale`); both hold a usable repo_root.
        if !name.starts_with("engine-") || !name.contains(".json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(desc) = serde_json::from_slice::<EngineDescriptor>(&bytes) else {
            continue;
        };
        // RFC3339 timestamps sort lexicographically in chronological order.
        let newer = newest
            .as_ref()
            .map(|cur| desc.started_at > cur.started_at)
            .unwrap_or(true);
        if newer {
            newest = Some(desc);
        }
    }
    newest
}

/// Every project darkrun knows about on this machine: registered `project.json`
/// records UNIONED with projects inferred from engine descriptors.
///
/// [`list_projects`] surfaces only projects that were explicitly registered (Add
/// a project, or the boot-time backfill in [`ensure_project_registered`]). But a
/// session an agent started before that backfill existed leaves only an engine
/// descriptor behind, never a `project.json`. This recovers those: for any slug
/// dir with no registry record, it synthesizes one from the newest engine
/// descriptor whose repo root still exists on disk, so the desktop surfaces every
/// repo you have actually run against, live or idle, without a manual add.
///
/// Worktree engine dirs collapse onto their canonical project (a linked worktree
/// resolves to the main checkout, [`slug_for`]), and a descriptor whose repo root
/// is gone (a deleted clone, a retired worktree with no surviving checkout) is
/// skipped, mirroring the prune in [`list_projects_in`].
#[cfg(not(tarpaulin_include))] // resolves the real home dir; logic via list_known_projects_in
pub fn list_known_projects() -> io::Result<Vec<ProjectRecord>> {
    match default_root() {
        Some(root) => list_known_projects_in(&root),
        None => Ok(Vec::new()),
    }
}

/// Like [`list_known_projects`] but scans an explicit `root`. Used by tests.
pub fn list_known_projects_in(root: &Path) -> io::Result<Vec<ProjectRecord>> {
    // The durable registry first (already pruned + identity-healed), keyed by
    // slug so a synthesized record never shadows a real registration.
    let mut by_slug: BTreeMap<String, ProjectRecord> = list_projects_in(root)?
        .into_iter()
        .map(|rec| (rec.slug.clone(), rec))
        .collect();

    let slug_dirs = match fs::read_dir(root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(by_slug.into_values().collect())
        }
        Err(e) => return Err(e),
    };
    for slug_entry in slug_dirs.flatten() {
        let slug_path = slug_entry.path();
        if !slug_path.is_dir() {
            continue;
        }
        let Some(desc) = newest_engine_descriptor_in(&slug_path) else {
            continue;
        };
        // The session ran against `repo_root`; a project IS its git repo, so
        // resolve a worktree to its main checkout, and skip it when that checkout
        // no longer exists (nothing left to open).
        let canonical = resolve_project_root(&desc.repo_root);
        if !canonical.exists() {
            continue;
        }
        let slug = slug_for(&canonical);
        by_slug.entry(slug.clone()).or_insert_with(|| ProjectRecord {
            slug,
            name: display_name_for(&canonical),
            path: canonical,
            added_at: Some(desc.started_at.clone()),
        });
    }
    Ok(by_slug.into_values().collect())
}

/// Ensure a durable [`ProjectRecord`] exists for `repo_root`, preserving any
/// record already there.
///
/// Called on engine boot so every session that runs makes its project visible to
/// the desktop, even one an agent started without ever touching Add a project. An
/// existing record wins (its `added_at` and display name are kept); only a missing
/// one is written. Best-effort: callers ignore the error.
#[cfg(not(tarpaulin_include))] // resolves the real home dir; logic via ensure_project_registered_in
pub fn ensure_project_registered(repo_root: &Path) -> io::Result<ProjectRecord> {
    let root = default_root().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not resolve home directory for the darkrun discovery registry",
        )
    })?;
    ensure_project_registered_in(&root, repo_root)
}

/// Like [`ensure_project_registered`] but under an explicit registry `root`. Used
/// by tests to point the tree at a temp dir.
pub fn ensure_project_registered_in(root: &Path, repo_root: &Path) -> io::Result<ProjectRecord> {
    let canonical = resolve_project_root(repo_root);
    let slug = slug_for(&canonical);
    if let Some(existing) = read_project_record_in(root, &slug)? {
        return Ok(existing);
    }
    register_project_in(root, repo_root, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn sample_addr() -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4317))
    }

    #[test]
    fn test_slug_derivation_simple() {
        // The basename leads the slug; a hash suffix follows.
        let slug = slug_for(Path::new("/Users/dev/darkrun"));
        assert!(slug.starts_with("darkrun-"), "slug was {slug}");
        // Hash suffix is 8 hex chars.
        let suffix = slug.strip_prefix("darkrun-").unwrap();
        assert_eq!(suffix.len(), 8);
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_slug_derivation_sanitizes() {
        // Spaces and special chars become dashes; same basename + different path
        // yields a different hash suffix (collision-resistant).
        let a = slug_for(Path::new("/home/alice/My App!"));
        let b = slug_for(Path::new("/home/bob/My App!"));
        assert!(a.starts_with("My-App-"), "slug was {a}");
        assert!(b.starts_with("My-App-"), "slug was {b}");
        assert_ne!(a, b, "different paths must not collide");
        // No illegal path characters survive.
        assert!(!a.contains(' '));
        assert!(!a.contains('!'));
        assert!(!a.contains('/'));
    }

    #[test]
    fn test_descriptor_roundtrip() {
        let descriptor = EngineDescriptor {
            pid: 4242,
            addr: sample_addr(),
            repo_root: PathBuf::from("/Users/dev/darkrun"),
            slug: "darkrun-deadbeef".to_string(),
            harness: "claude".to_string(),
            started_at: "2026-05-31T00:00:00+00:00".to_string(),
            reachability: darkrun_api::tunnel::Reachability {
                local: Some(darkrun_api::tunnel::LocalCandidate {
                    host: "127.0.0.1".into(),
                    port: 4317,
                }),
                relay: None,
            },
        };
        let json = serde_json::to_vec(&descriptor).unwrap();
        let back: EngineDescriptor = serde_json::from_slice(&json).unwrap();
        assert_eq!(descriptor, back);
        // A local-only engine advertises just the loopback candidate.
        assert!(back.reachability.local.is_some());
        assert!(back.reachability.relay.is_none());
    }

    #[test]
    fn announce_with_relay_advertises_both_candidates() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = EngineRegistry::with_root(tmp.path(), "/Users/dev/some-repo").with_relay(
            darkrun_api::tunnel::RelayCandidate {
                url: "wss://relay.darkrun.ai".into(),
                session: "some-repo".into(),
            },
        );
        let d = registry.announce(sample_addr(), "claude").unwrap();
        // Local candidate is the loopback addr; relay candidate is advertised too.
        let local = d.reachability.local.expect("local candidate");
        assert_eq!(local.port, 4317);
        let relay = d.reachability.relay.expect("relay candidate");
        assert_eq!(relay.url, "wss://relay.darkrun.ai");
        assert_eq!(relay.session, "some-repo");
    }

    #[test]
    fn test_stale_detection() {
        // The current process is alive; an almost-certainly-dead pid is not.
        assert!(process_alive(std::process::id()));
        // A very large pid is exceedingly unlikely to be a live process.
        assert!(!process_alive(0x7fff_fffe));
    }

    #[test]
    fn test_announce_and_list_live() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = EngineRegistry::with_root(tmp.path(), "/Users/dev/some-repo");
        let descriptor = registry.announce(sample_addr(), "claude").unwrap();

        assert!(registry.descriptor_path().exists());
        assert_eq!(descriptor.addr, sample_addr());

        // The live reader returns the descriptor while this process is alive.
        let live = list_live_engines_in(tmp.path()).unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].pid, std::process::id());
        assert_eq!(live[0].addr, sample_addr());
    }

    #[test]
    fn test_mark_stale_retains_record() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = EngineRegistry::with_root(tmp.path(), "/Users/dev/another-repo");
        registry.announce(sample_addr(), "claude").unwrap();
        let live_path = registry.descriptor_path();
        assert!(live_path.exists());

        registry.mark_stale().unwrap();

        // Live descriptor is gone, but the record is RETAINED as `.stale`.
        assert!(!live_path.exists());
        let stale = stale_path(&live_path);
        assert!(stale.exists(), "stale record must be retained, not deleted");

        // A stale descriptor is no longer returned by the live reader.
        let live = list_live_engines_in(tmp.path()).unwrap();
        assert!(live.is_empty());

        // mark_stale is idempotent.
        registry.mark_stale().unwrap();
    }

    #[test]
    fn test_register_project_writes_record() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = "/Users/dev/storefront";
        let record =
            register_project_in(tmp.path(), Path::new(repo), Some("Storefront".to_string()))
                .unwrap();

        // The record's slug matches the slug the engine would derive, and the
        // file lands in that slug directory.
        assert_eq!(record.slug, slug_for(Path::new(repo)));
        assert_eq!(record.path, PathBuf::from(repo));
        assert_eq!(record.name.as_deref(), Some("Storefront"));
        assert!(record.added_at.is_some(), "added_at should be stamped");

        let on_disk = tmp.path().join(&record.slug).join(PROJECT_RECORD_FILE);
        assert!(on_disk.exists(), "project.json should be written");
    }

    #[test]
    fn test_read_project_record_roundtrip_and_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = "/Users/dev/api-gateway";
        let written = register_project_in(tmp.path(), Path::new(repo), None).unwrap();

        // Reading by slug returns the same record.
        let read = read_project_record_in(tmp.path(), &written.slug)
            .unwrap()
            .expect("record should exist");
        assert_eq!(read, written);

        // A slug with no record reads as None, not an error.
        let absent = read_project_record_in(tmp.path(), "no-such-slug-deadbeef").unwrap();
        assert!(absent.is_none());
    }

    #[test]
    fn test_list_projects_scans_all_and_skips_non_records() {
        let tmp = tempfile::tempdir().unwrap();
        let alpha = tempfile::tempdir().unwrap();
        let beta = tempfile::tempdir().unwrap();
        register_project_in(tmp.path(), alpha.path(), None).unwrap();
        register_project_in(tmp.path(), beta.path(), None).unwrap();

        // A legacy slug dir with only an engine descriptor (no project.json) and
        // a slug dir with a malformed record must both be skipped, not crash.
        let engine_only = EngineRegistry::with_root(tmp.path(), "/Users/dev/engine-only");
        engine_only.announce(sample_addr(), "claude").unwrap();

        let bad_dir = tmp.path().join("garbage-00000000");
        fs::create_dir_all(&bad_dir).unwrap();
        fs::write(bad_dir.join(PROJECT_RECORD_FILE), b"not json").unwrap();

        let projects = list_projects_in(tmp.path()).unwrap();
        assert_eq!(projects.len(), 2, "only valid records: {projects:?}");
        let paths: Vec<&PathBuf> = projects.iter().map(|p| &p.path).collect();
        assert!(paths.contains(&&alpha.path().to_path_buf()));
        assert!(paths.contains(&&beta.path().to_path_buf()));
    }

    #[test]
    fn test_list_projects_empty_tree() {
        let tmp = tempfile::tempdir().unwrap();
        // Point at a non-existent subdir: an absent tree lists empty, not error.
        let missing = tmp.path().join("never-created");
        assert!(list_projects_in(&missing).unwrap().is_empty());
    }

    #[test]
    fn test_register_then_list_overlays_with_live_engine() {
        // A project registered then served by a live engine for the SAME repo
        // shares a slug — proving the desktop can overlay live status on the
        // project record by slug match.
        let tmp = tempfile::tempdir().unwrap();
        let repo_dir = tempfile::tempdir().unwrap();
        let repo = repo_dir.path().to_str().unwrap();
        let record = register_project_in(tmp.path(), repo_dir.path(), None).unwrap();

        let engine = EngineRegistry::with_root(tmp.path(), repo);
        engine.announce(sample_addr(), "claude").unwrap();

        let projects = list_projects_in(tmp.path()).unwrap();
        let live = list_live_engines_in(tmp.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(live.len(), 1);
        // Same slug => the overlay key matches.
        assert_eq!(projects[0].slug, live[0].slug);
        assert_eq!(record.slug, live[0].slug);
    }

    #[test]
    fn list_live_engines_in_skips_junk_and_handles_missing() {
        // An absent root lists empty (NotFound), not an error.
        let tmp = tempfile::tempdir().unwrap();
        assert!(list_live_engines_in(&tmp.path().join("never")).unwrap().is_empty());

        // A non-dir entry at the top level is skipped; a slug dir with only
        // non-descriptor / malformed files yields nothing live.
        fs::write(tmp.path().join("loose-file"), b"x").unwrap();
        let slug = tmp.path().join("darkrun-deadbeef");
        fs::create_dir_all(&slug).unwrap();
        fs::write(slug.join("notes.txt"), b"not a descriptor").unwrap();
        fs::write(slug.join("engine-stale.json.stale"), b"{}").unwrap();
        fs::write(slug.join("engine-bad.json"), b"not json").unwrap(); // parse-skip
        assert!(list_live_engines_in(tmp.path()).unwrap().is_empty());

        assert!(!is_live_descriptor_name(Path::new("engine-x.json.stale")));
        assert!(is_live_descriptor_name(Path::new("/a/engine-x.json")));
    }

    #[test]
    fn list_live_engines_in_skips_an_unreadable_slug_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        // A slug dir that stats as a directory but can't be read (perms) → the
        // inner read_dir errors and that slug is skipped, not fatal.
        let slug = tmp.path().join("darkrun-locked");
        fs::create_dir_all(&slug).unwrap();
        fs::set_permissions(&slug, fs::Permissions::from_mode(0o000)).unwrap();
        let live = list_live_engines_in(tmp.path());
        fs::set_permissions(&slug, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(live.unwrap().is_empty(), "an unreadable slug dir is skipped");
    }

    #[test]
    fn home_rooted_wrappers_resolve_under_an_overridden_home() {
        // Override HOME so the home-based wrappers (new / default_root /
        // register_project / list_projects / list_live_engines) operate under a
        // throwaway tree instead of the real ~/.darkrun.
        let _g = HOME_LOCK.lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("HOME");
        std::env::set_var("HOME", home.path());

        let root = default_root().expect("home resolves");
        assert!(root.ends_with(".darkrun"));

        let widget = tempfile::tempdir().unwrap();
        let reg = EngineRegistry::new(widget.path().to_str().unwrap())
            .expect("registry under home");
        reg.announce(sample_addr(), "claude").unwrap();
        assert!(!list_live_engines().unwrap().is_empty());

        let rec = register_project(widget.path(), Some("Widget".into())).unwrap();
        assert_eq!(rec.name.as_deref(), Some("Widget"));
        assert!(list_projects().unwrap().iter().any(|p| p.slug == rec.slug));

        match prev {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn registry_and_clone_roots_share_a_base() {
        let _g = HOME_LOCK.lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("HOME");
        std::env::set_var("HOME", home.path());

        let reg = default_root().expect("home resolves");
        let clones = default_clone_root().expect("home resolves");
        // Discovery registry (`.darkrun`) and clones (`darkrun`) are siblings,
        // so they sit in one shared data home (the app-group container on macOS).
        assert!(reg.ends_with(".darkrun"));
        assert!(clones.ends_with("darkrun"));
        assert_eq!(reg.parent(), clones.parent());
        #[cfg(target_os = "macos")]
        assert!(
            reg.to_string_lossy().contains("Group Containers/group.ai.darkrun"),
            "macOS roots live in the app-group container: {reg:?}"
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(reg.parent(), Some(home.path()));

        match prev {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn slug_for_falls_back_when_basename_is_all_special() {
        // An all-non-alphanumeric basename sanitizes to dashes → the "repo" fallback.
        let slug = slug_for(Path::new("/x/@@@"));
        assert!(slug.starts_with("repo-"), "got {slug}");
    }

    #[test]
    fn is_live_descriptor_name_rejects_nameless_and_stale_paths() {
        assert!(is_live_descriptor_name(Path::new("/d/engine-7.json")));
        assert!(!is_live_descriptor_name(Path::new("/d/engine-7.json.stale")));
        // A path with no final component → the no-filename guard.
        assert!(!is_live_descriptor_name(Path::new("..")));
    }

    #[test]
    fn list_live_engines_in_surfaces_a_non_notfound_read_error() {
        // Pointing at a FILE (not a dir) makes read_dir fail with a non-NotFound
        // error, exercising the error-propagation arm.
        let f = tempfile::NamedTempFile::new().unwrap();
        assert!(list_live_engines_in(f.path()).is_err());
    }

    #[test]
    fn list_projects_in_surfaces_a_non_notfound_read_error() {
        let f = tempfile::NamedTempFile::new().unwrap();
        assert!(list_projects_in(f.path()).is_err());
    }

    #[test]
    fn read_project_record_errors_when_the_record_is_a_directory() {
        // A `project.json` that is itself a directory makes fs::read fail with a
        // non-NotFound error → the hard-error arm (vs. Ok(None) for absent).
        let dir = tempfile::tempdir().unwrap();
        let slug = "s";
        std::fs::create_dir_all(dir.path().join(slug).join(PROJECT_RECORD_FILE)).unwrap();
        assert!(read_project_record_in(dir.path(), slug).is_err());
        // And Ok(None) when truly absent.
        assert!(read_project_record_in(dir.path(), "ghost").unwrap().is_none());
    }

    #[test]
    fn list_projects_heals_a_stale_worktree_keyed_record() {
        use std::process::Command;
        // A real repo with a linked worktree under .claude/worktrees/.
        let repo = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            assert!(Command::new("git").arg("-C").arg(repo.path()).args(args)
                .status().unwrap().success());
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@darkrun.ai"]);
        git(&["config", "user.name", "t"]);
        std::fs::write(repo.path().join("README.md"), "x\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "init"]);
        let wt = repo.path().join(".claude/worktrees/floofy");
        git(&["worktree", "add", "-q", wt.to_str().unwrap()]);

        // A STALE registration: keyed by the worktree dir, pointing at it.
        let reg = tempfile::tempdir().unwrap();
        let stale_slug = format!("floofy-{}", short_hash(&wt));
        let stale = ProjectRecord {
            slug: stale_slug.clone(),
            path: wt.clone(),
            name: Some("floofy".into()),
            added_at: Some("2026-06-09T00:00:00Z".into()),
        };
        write_project_record_in(reg.path(), &stale_slug, &stale).unwrap();
        // ...with an engine descriptor file beside it.
        std::fs::write(reg.path().join(&stale_slug).join("engine-1.json"), "{}").unwrap();

        let projects = list_projects_in(reg.path()).unwrap();
        assert_eq!(projects.len(), 1, "{projects:?}");
        let healed = &projects[0];
        let canonical = resolve_project_root(&wt);
        assert_eq!(healed.path, canonical, "record points at the MAIN checkout");
        assert_eq!(healed.slug, slug_for(&canonical), "keyed by the canonical slug");
        assert_ne!(healed.slug, stale_slug);
        assert!(
            healed.name.as_deref() != Some("floofy"),
            "a worktree-directory display name does not survive: {:?}",
            healed.name
        );
        // The stale dir is retired; the descriptor moved with the project.
        assert!(!reg.path().join(&stale_slug).exists());
        assert!(reg.path().join(&healed.slug).join("engine-1.json").exists());
        // A second list is stable (idempotent heal).
        let again = list_projects_in(reg.path()).unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].slug, healed.slug);
    }

    #[test]
    fn register_project_canonicalizes_worktrees_and_names_by_repo() {
        use std::process::Command;
        let repo = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            assert!(Command::new("git").arg("-C").arg(repo.path()).args(args)
                .status().unwrap().success());
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@darkrun.ai"]);
        git(&["config", "user.name", "t"]);
        git(&["remote", "add", "origin", "git@github.com:acme/widgets.git"]);
        std::fs::write(repo.path().join("a"), "x").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "init"]);
        let wt = repo.path().join(".claude/worktrees/scratch");
        git(&["worktree", "add", "-q", wt.to_str().unwrap()]);

        let reg = tempfile::tempdir().unwrap();
        let rec = register_project_in(reg.path(), &wt, None).unwrap();
        assert_eq!(rec.path, resolve_project_root(&wt), "registers the repo, not the worktree");
        assert_eq!(rec.name.as_deref(), Some("widgets"), "named by the origin repo");
        assert!(rec.slug.starts_with("widgets-"), "{}", rec.slug);
    }

    #[test]
    fn list_known_projects_in_recovers_a_session_that_was_never_registered() {
        let reg = tempfile::tempdir().unwrap();
        // An existing repo dir a session ran against.
        let repo = tempfile::tempdir().unwrap();
        // The engine wrote its descriptor, then died (only a `.stale` record left).
        // Crucially, NO project.json was ever written.
        let registry = EngineRegistry::with_root(reg.path(), repo.path());
        registry.announce(sample_addr(), "claude").unwrap();
        registry.mark_stale().unwrap();

        // The durable registry alone sees nothing (no project.json).
        assert!(list_projects_in(reg.path()).unwrap().is_empty());

        // The union recovers the project from the historical engine descriptor.
        let known = list_known_projects_in(reg.path()).unwrap();
        assert_eq!(known.len(), 1, "the run's repo should surface as a project");
        assert_eq!(known[0].slug, slug_for(&resolve_project_root(repo.path())));
        assert!(known[0].path.exists(), "the synthesized path is a real checkout");
    }

    #[test]
    fn list_known_projects_in_does_not_duplicate_a_registered_project() {
        let reg = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        // Explicitly registered AND has an engine descriptor for the same repo.
        register_project_in(reg.path(), repo.path(), Some("Reg".to_string())).unwrap();
        EngineRegistry::with_root(reg.path(), repo.path())
            .announce(sample_addr(), "claude")
            .unwrap();

        let known = list_known_projects_in(reg.path()).unwrap();
        assert_eq!(known.len(), 1, "one repo yields one project, not a duplicate");
        assert_eq!(known[0].name.as_deref(), Some("Reg"), "the registration wins");
    }

    #[test]
    fn ensure_project_registered_in_writes_once_then_preserves() {
        let reg = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        // First call materializes a fresh record.
        let first = ensure_project_registered_in(reg.path(), repo.path()).unwrap();
        assert!(first.added_at.is_some(), "a new registration is stamped");
        // A second call must return the SAME record, never rewriting added_at.
        let second = ensure_project_registered_in(reg.path(), repo.path()).unwrap();
        assert_eq!(first, second, "an existing record is preserved, not rewritten");
    }
}
