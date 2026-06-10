//! The `/browse` remote workspace source — **GraphQL, client-side, both hosts**.
//!
//! `/browse` renders a *published* darkrun workspace read-only, entirely in the
//! browser, the way the predecessor did: the token lives client-side (see
//! [`crate::auth`]) and the GraphQL calls go straight from the page to the host,
//! so neither the token nor the browsed data ever flows through darkrun's
//! servers.
//!
//! ## Why GraphQL
//!
//! One query fetches a run's WHOLE workspace — the `darkrun/<slug>/main` branch
//! list, and every file's contents inline — instead of dozens of REST round
//! trips. It is also the only way GitHub exposes its API to an authenticated
//! browser app (GitHub forbids anonymous GraphQL; GitLab allows anonymous
//! GraphQL for public projects).
//!
//! - **GitHub** — `object(expression:"<ref>:<path>")` returns a Blob's `text` or
//!   a Tree's `entries` inline. The run list is two queries (branch/tree
//!   discovery, then one aliased query for every run's `run.md`/`state.json`); a
//!   run's detail is a single query.
//! - **GitLab** — `repository.blobs(ref:, paths:)` returns many blobs'
//!   `rawTextBlob` in one call; `branchNames`/`tree` discover runs and paths.
//!
//! ## The right branches
//!
//! A run's authoritative state lives on its **run-main branch**,
//! `darkrun/<slug>/main`, while in flight; browse reads each run from its own
//! branch and falls back to the default branch for sealed-and-merged runs. Files
//! parse with the engine's own [`darkrun_core::frontmatter`] into the real
//! [`RunFrontmatter`]/[`UnitFrontmatter`]; `state.json` into [`RunState`]; status
//! comes from [`darkrun_core::derive::station_status`] — so the website derives
//! exactly what the engine does.

use std::collections::BTreeMap;

use darkrun_core::derive::station_status;
use darkrun_core::domain::{RunFrontmatter, StationPhase, Status, UnitFrontmatter};
use darkrun_core::frontmatter;
use darkrun_core::RunState;
use darkrun_ui::prelude::{
    GraphEdge, Phase, RunCardData, StationStatus, Tone, UnitGraphNode,
};
use serde::Deserialize;

/// The engine's branch namespace — run-main branches are `darkrun/<slug>/main`
/// (mirrors `darkrun_mcp::lifecycle::BRANCH_PREFIX`).
const BRANCH_PREFIX: &str = "darkrun";

/// A run's run-main branch name: `darkrun/<slug>/main`.
fn run_main_branch(slug: &str) -> String {
    format!("{BRANCH_PREFIX}/{slug}/main")
}

/// A station's working branch: `darkrun/<slug>/<station>` (mirrors
/// `darkrun_mcp::lifecycle::station_branch`). The CURRENT station's live,
/// not-yet-landed work lives here; completed stations have landed to run-main.
fn station_branch(slug: &str, station: &str) -> String {
    format!("{BRANCH_PREFIX}/{slug}/{station}")
}

/// Given the branch tails under `darkrun/` (i.e. `<slug>/main`,
/// `<slug>/<station>`, `<slug>/units/…`), choose ONE ref per run slug to read
/// its `run.md`/`state.json` from: the **run-main** branch when it exists, else a
/// **station branch**.
///
/// This is what makes a brand-new run discoverable: before its first station
/// lands, the run has NO `darkrun/<slug>/main` branch and nothing on the default
/// branch — its state lives only on `darkrun/<slug>/<station>`. Matching just
/// `*/main` would miss it entirely.
fn runs_from_branch_tails<I: IntoIterator<Item = String>>(tails: I) -> BTreeMap<String, String> {
    let mut mains: std::collections::BTreeSet<String> = Default::default();
    let mut station: BTreeMap<String, String> = BTreeMap::new();
    for tail in tails {
        let mut it = tail.splitn(2, '/');
        let slug = match it.next() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let Some(rest) = it.next() else { continue };
        if rest == "main" {
            mains.insert(slug);
        } else if !rest.contains('/') {
            // A 2-part station branch `darkrun/<slug>/<station>`. Deeper branches
            // (`units/…`) don't add a new read ref — the slug is already covered.
            station
                .entry(slug.clone())
                .or_insert_with(|| format!("{BRANCH_PREFIX}/{slug}/{rest}"));
        }
    }
    let mut runs: BTreeMap<String, String> = BTreeMap::new();
    for slug in mains {
        let main = run_main_branch(&slug);
        runs.insert(slug, main);
    }
    for (slug, station_ref) in station {
        runs.entry(slug).or_insert(station_ref);
    }
    runs
}

// ---------------------------------------------------------------------------
// Repo reference + host provider.
// ---------------------------------------------------------------------------

/// A resolved reference to a public repository hosting a `.darkrun/` workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    /// The host, e.g. `github.com` or `gitlab.com`.
    pub host: String,
    /// The owner / org segment.
    pub owner: String,
    /// The repository name (may contain `/` for nested groups on GitLab).
    pub repo: String,
}

impl RepoRef {
    /// `owner/repo` — the human-facing repository path.
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    /// The `/browse/...` URL path that addresses this repo's run list.
    pub fn browse_path(&self) -> String {
        format!("/browse/{}/{}/{}", self.host, self.owner, self.repo)
    }

    /// The `/browse/...` URL path that addresses one run inside this repo.
    pub fn run_path(&self, slug: &str) -> String {
        format!("{}/run/{}", self.browse_path(), slug)
    }

    /// The catch-all route segments addressing this repo's run list.
    pub fn list_rest(&self) -> Vec<String> {
        let mut v = vec![self.host.clone(), self.owner.clone()];
        v.extend(self.repo.split('/').map(|s| s.to_string()));
        v
    }

    /// The catch-all route segments addressing one run inside this repo.
    pub fn run_rest(&self, slug: &str) -> Vec<String> {
        let mut v = self.list_rest();
        v.push("run".to_string());
        v.push(slug.to_string());
        v
    }

    /// The host provider, or [`BrowseError::UnsupportedHost`] when unknown.
    fn provider(&self) -> Result<Provider, BrowseError> {
        Provider::from_host(&self.host)
            .ok_or_else(|| BrowseError::UnsupportedHost(self.host.clone()))
    }

    /// The GitLab `fullPath` project id (`owner/repo`, nested groups included).
    fn gl_full_path(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }
}

/// A supported repository host. Mirrors `darkrun_vcs::Provider::from_host`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provider {
    GitHub,
    GitLab,
}

impl Provider {
    fn from_host(host: &str) -> Option<Provider> {
        if host == "github.com" || host.ends_with(".github.com") {
            Some(Provider::GitHub)
        } else if host == "gitlab.com" || host.contains("gitlab") {
            Some(Provider::GitLab)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// URL → target parsing (pure).
// ---------------------------------------------------------------------------

/// What a `/browse/...` URL addresses: a repo's run list, or one run's detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `/browse/{host}/{owner}/{repo}` — list the repo's runs.
    RunList(RepoRef),
    /// `/browse/{host}/{owner}/{repo}/run/{slug}` — one run's detail.
    Run(RepoRef, String),
}

impl Target {
    /// Parse the catch-all `rest` segments (everything after `/browse/`) into a
    /// [`Target`]. Returns `None` without enough segments (host + owner + repo).
    pub fn parse(rest: &[String]) -> Option<Target> {
        let rest: Vec<&str> = rest.iter().map(|s| s.as_str()).filter(|s| !s.is_empty()).collect();
        if let Some(i) = rest.iter().position(|s| *s == "run") {
            let repo = Self::repo_ref(&rest[..i])?;
            let slug = rest.get(i + 1).filter(|s| !s.is_empty())?;
            Some(Target::Run(repo, (*slug).to_string()))
        } else {
            Some(Target::RunList(Self::repo_ref(&rest)?))
        }
    }

    fn repo_ref(segs: &[&str]) -> Option<RepoRef> {
        match segs {
            [host, owner, repo_rest @ ..] if !repo_rest.is_empty() => Some(RepoRef {
                host: (*host).to_string(),
                owner: (*owner).to_string(),
                repo: repo_rest.join("/"),
            }),
            _ => None,
        }
    }

    /// The repo this target lives in.
    pub fn repo(&self) -> &RepoRef {
        match self {
            Target::RunList(r) | Target::Run(r, _) => r,
        }
    }
}

/// Normalize a user-pasted repository reference into `/browse`-ready path
/// segments. Accepts `https://github.com/org/repo`, `gitlab.com/group/sub/proj`,
/// a `.git` suffix, an `scp`-style `git@host:owner/repo`, with or without a
/// trailing slash. Returns `[host, owner, repo...]`, or `None`.
pub fn normalize_repo_input(raw: &str) -> Option<Vec<String>> {
    let mut s = raw.trim();
    for scheme in ["https://", "http://", "git://", "ssh://", "git@"] {
        if let Some(rest) = s.strip_prefix(scheme) {
            s = rest;
            break;
        }
    }
    let s = s.replace(':', "/");
    let s = s.trim().trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    let segs: Vec<String> = s
        .split('/')
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string())
        .collect();
    if segs.len() >= 3 && segs[0].contains('.') {
        Some(segs)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Errors.
// ---------------------------------------------------------------------------

/// An error surfaced to the browse UI. Each variant carries a message the page
/// renders directly.
#[derive(Debug, Clone)]
pub enum BrowseError {
    /// The host isn't one we know how to read.
    UnsupportedHost(String),
    /// This host's GraphQL requires a signed-in token (GitHub).
    NeedsAuth(&'static str),
    /// A network/transport failure (the fetch itself failed).
    Network(String),
    /// The host answered with a non-success status.
    Status { code: u16, what: String },
    /// A response existed but couldn't be parsed.
    Parse(String),
    /// The host accepted the request but the GraphQL query failed (e.g. an org
    /// token policy, a missing scope, or no access to the repo).
    Graphql(String),
    /// The repo has no `.darkrun/` workspace (or no runs in it).
    NoWorkspace,
}

impl std::fmt::Display for BrowseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowseError::UnsupportedHost(h) => {
                write!(f, "Browsing {h} isn't supported — only github.com and gitlab.com.")
            }
            BrowseError::NeedsAuth(host) => write!(
                f,
                "Connect {host} to browse — {host}'s API has no anonymous access. \
                 Use the button above; your token stays in this browser."
            ),
            BrowseError::Network(e) => write!(f, "Couldn't reach the repository: {e}"),
            BrowseError::Status { code, what } => match code {
                401 | 403 => write!(
                    f,
                    "The host rejected the request ({code}). The repository may be private — \
                     sign in above — or the host rate-limited this IP."
                ),
                429 => write!(f, "The host rate-limited the request. Try again shortly."),
                404 => write!(f, "Not found: {what}. Is the repository public?"),
                _ => write!(f, "The host returned {code} for {what}."),
            },
            BrowseError::Parse(e) => write!(f, "Couldn't read the workspace: {e}"),
            BrowseError::Graphql(m) => write!(f, "{m}"),
            BrowseError::NoWorkspace => write!(
                f,
                "This repository has no published .darkrun/ workspace (or no runs in it)."
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// GraphQL transport.
// ---------------------------------------------------------------------------

/// POST a GraphQL query and decode `data`, mapping transport/`errors` failures.
/// `token` adds a bearer header (required for GitHub, optional for GitLab).
///
/// GraphQL reports query-level failures as **HTTP 200 with `data: null` and an
/// `errors` array** (e.g. an org forbidding a long-lived token). So we parse
/// loosely first and surface `errors[0].message` directly — a strict typed
/// decode would otherwise choke on the null `data` with a useless message.
async fn graphql<T: serde::de::DeserializeOwned>(
    url: &str,
    token: Option<&str>,
    query: &str,
    variables: serde_json::Value,
) -> Result<T, BrowseError> {
    let body = serde_json::json!({ "query": query, "variables": variables });
    let mut req = gloo_net::http::Request::post(url);
    if let Some(t) = token {
        req = req.header("Authorization", &format!("Bearer {t}"));
    }
    let resp = req
        .json(&body)
        .map_err(|e| BrowseError::Network(e.to_string()))?
        .send()
        .await
        .map_err(|e| BrowseError::Network(e.to_string()))?;
    if !resp.ok() {
        return Err(BrowseError::Status { code: resp.status(), what: url.to_string() });
    }
    let text = resp.text().await.map_err(|e| BrowseError::Network(e.to_string()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| BrowseError::Parse(e.to_string()))?;
    if let Some(msg) = value
        .get("errors")
        .and_then(|e| e.as_array())
        .and_then(|a| a.first())
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
    {
        return Err(BrowseError::Graphql(msg.to_string()));
    }
    let data = value.get("data").cloned().unwrap_or(serde_json::Value::Null);
    serde_json::from_value::<T>(data).map_err(|e| BrowseError::Parse(e.to_string()))
}

/// JSON-encode `s` as a GraphQL string literal (for embedding into a query).
fn gql_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
}

// ---------------------------------------------------------------------------
// Derivation: parsed frontmatter + state.json → darkrun-ui view-models.
// ---------------------------------------------------------------------------

/// Map a lifecycle [`Status`] onto its snake_case wire string.
fn status_str(s: Status) -> &'static str {
    match s {
        Status::Pending => "pending",
        Status::Active => "active",
        Status::InProgress => "in_progress",
        Status::Completed => "completed",
        Status::Blocked => "blocked",
    }
}

/// Map a lifecycle [`Status`] onto a UI [`Tone`].
fn status_tone(s: Status) -> Tone {
    match s {
        Status::Completed => Tone::Ok,
        Status::Active | Status::InProgress => Tone::Info,
        Status::Blocked => Tone::Danger,
        Status::Pending => Tone::Neutral,
    }
}

/// Map an engine [`StationPhase`] onto the UI [`Phase`]. The pre-execution user
/// gate reads as `Review` (its subheader twin).
fn station_phase_ui(p: StationPhase) -> Phase {
    match p {
        StationPhase::Spec => Phase::Spec,
        StationPhase::Review | StationPhase::UserGate => Phase::Review,
        StationPhase::Manufacture => Phase::Manufacture,
        StationPhase::Audit => Phase::Audit,
        StationPhase::Reflect => Phase::Reflect,
        StationPhase::Checkpoint => Phase::Checkpoint,
    }
}

/// The ordered station plan a run walks: its recorded `plan`, else the factory's
/// declared stations, else the hardcoded FSSBPH spine.
fn ordered_stations(state: &RunState, factory: &str) -> Vec<String> {
    if !state.plan.is_empty() {
        return state.plan.clone();
    }
    if let Ok(f) = darkrun_content::load_validated(factory) {
        let names: Vec<String> = f.stations.iter().map(|s| s.name().to_string()).collect();
        if !names.is_empty() {
            return names;
        }
    }
    ["frame", "specify", "shape", "build", "prove", "harden"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// One station's projected display state on the assembly line.
#[derive(Debug, Clone, PartialEq)]
pub struct StationView {
    /// Station name.
    pub name: String,
    /// Done / current / pending — the same derivation the engine runs.
    pub status: StationStatus,
    /// The active phase within this station (meaningful for the current one).
    pub phase: Option<Phase>,
}

/// Project the run's ordered stations into [`StationView`]s, mirroring the
/// desktop's `map::station_items`.
fn station_views(state: &RunState, factory: &str) -> Vec<StationView> {
    let ordered = ordered_stations(state, factory);
    let active_index = ordered.iter().position(|n| n == &state.active_station);
    ordered
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let st = state.stations.get(name);
            let explicit_done =
                st.map(|s| matches!(s.status, Status::Completed)).unwrap_or(false);
            let status = if explicit_done {
                StationStatus::Done
            } else {
                match station_status(i, active_index) {
                    Status::Completed => StationStatus::Done,
                    Status::Active => StationStatus::Current,
                    _ => StationStatus::Pending,
                }
            };
            let phase = st.map(|s| station_phase_ui(s.phase));
            StationView { name: name.clone(), status, phase }
        })
        .collect()
}

/// Build a [`RunCardData`] from a run's frontmatter + (optional) state.
fn run_card(slug: &str, fm: &RunFrontmatter, body: &str, state: Option<&RunState>) -> RunCardData {
    let title = fm
        .title
        .clone()
        .filter(|t| !t.is_empty())
        .or_else(|| frontmatter::first_heading(body))
        .unwrap_or_else(|| slug.to_string());
    let factory = if fm.factory.is_empty() {
        state.map(|s| s.factory.clone()).unwrap_or_default()
    } else {
        fm.factory.clone()
    };
    let active_station = state
        .map(|s| s.active_station.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fm.active_station.clone());
    let (phase, completed, total) = match state {
        Some(state) => {
            let views = station_views(state, &factory);
            let total = views.len() as u32;
            let completed = views.iter().filter(|v| v.status == StationStatus::Done).count() as u32;
            let phase = views
                .iter()
                .find(|v| v.status == StationStatus::Current)
                .and_then(|v| v.phase);
            (phase, completed, total)
        }
        None => (None, 0, 0),
    };
    RunCardData {
        slug: slug.to_string(),
        title,
        factory,
        active_station,
        phase,
        status: status_str(fm.status).to_string(),
        completed,
        total,
    }
}

/// A unit's projected row for the detail view.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitRowView {
    /// Unit slug (the node id).
    pub slug: String,
    /// Display title.
    pub title: String,
    /// Free-form unit type label.
    pub unit_type: String,
    /// Status badge tone.
    pub tone: Tone,
    /// Status display string.
    pub status: String,
    /// Pass (iteration) count.
    pub passes: u32,
    /// The units this one depends on.
    pub depends_on: Vec<String>,
}

/// A feedback item's projected row for the detail view.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedbackRowView {
    /// Stable id (e.g. `FB-03`).
    pub id: String,
    /// Title / summary.
    pub title: String,
    /// Status display string.
    pub status: String,
    /// Severity display string, if classified.
    pub severity: Option<String>,
    /// Author handle.
    pub author: String,
}

/// Minimal feedback frontmatter — only the fields the row renders.
#[derive(Debug, Default, Deserialize)]
struct FeedbackFm {
    #[serde(default)]
    feedback_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    author: Option<String>,
}

/// Project one unit's frontmatter + body into a row.
fn unit_row(slug: &str, fm: &UnitFrontmatter, body: &str) -> UnitRowView {
    let title = fm
        .name
        .clone()
        .filter(|t| !t.is_empty())
        .or_else(|| frontmatter::first_heading(body))
        .unwrap_or_else(|| slug.to_string());
    UnitRowView {
        slug: slug.to_string(),
        title,
        unit_type: fm.unit_type.clone(),
        tone: status_tone(fm.status),
        status: status_str(fm.status).to_string(),
        passes: fm.iterations.len() as u32,
        depends_on: fm.depends_on.clone(),
    }
}

/// The file stem of a `units/<slug>.md` / `feedback/<id>.md` path.
fn file_stem(path: &str) -> String {
    path.rsplit('/')
        .next()
        .and_then(|f| f.strip_suffix(".md"))
        .unwrap_or(path)
        .to_string()
}

/// Build a feedback row from a parsed `feedback/*.md`.
fn feedback_row(path: &str, fm: FeedbackFm) -> FeedbackRowView {
    FeedbackRowView {
        id: if fm.feedback_id.is_empty() { file_stem(path) } else { fm.feedback_id },
        title: fm.title,
        status: fm.status.unwrap_or_default(),
        severity: fm.severity,
        author: fm.author.unwrap_or_default(),
    }
}

/// Everything the run-detail page renders.
#[derive(Debug, Clone, PartialEq)]
pub struct RunDetail {
    /// The run slug.
    pub slug: String,
    /// Resolved display title.
    pub title: String,
    /// Factory name.
    pub factory: String,
    /// Review mode display string (team/solo/dark).
    pub mode: String,
    /// Lifecycle status string.
    pub status: String,
    /// The branch the state was read from (run-main branch, or the default).
    pub source_ref: String,
    /// RFC3339 start timestamp, if recorded.
    pub started_at: Option<String>,
    /// The run document body, rendered to HTML.
    pub body_html: String,
    /// The assembly-line stations.
    pub stations: Vec<StationView>,
    /// The active station's phase, for the secondary pipeline subheader.
    pub active_phase: Option<Phase>,
    /// Unit rows, in dependency-then-name order.
    pub units: Vec<UnitRowView>,
    /// Feedback rows.
    pub feedback: Vec<FeedbackRowView>,
}

impl RunDetail {
    /// The unit DAG nodes for [`darkrun_ui::prelude::UnitGraph`].
    pub fn graph_nodes(&self) -> Vec<UnitGraphNode> {
        self.units
            .iter()
            .map(|u| UnitGraphNode::new(u.slug.clone(), u.title.clone()).with_tone(u.tone))
            .collect()
    }

    /// The unit DAG edges (`from` depends-on → `to` dependent), restricted to
    /// edges whose endpoints both exist as nodes.
    pub fn graph_edges(&self) -> Vec<GraphEdge> {
        let ids: std::collections::BTreeSet<&str> =
            self.units.iter().map(|u| u.slug.as_str()).collect();
        let mut edges = Vec::new();
        for u in &self.units {
            for dep in &u.depends_on {
                if ids.contains(dep.as_str()) {
                    edges.push(GraphEdge { from: dep.clone(), to: u.slug.clone() });
                }
            }
        }
        edges
    }

    /// Whether the run has any units to graph.
    pub fn has_units(&self) -> bool {
        !self.units.is_empty()
    }
}

/// Assemble a [`RunDetail`] from already-fetched parts — shared by both hosts.
fn assemble_detail(
    slug: &str,
    fm: &RunFrontmatter,
    body: &str,
    state: Option<&RunState>,
    mut units: Vec<UnitRowView>,
    feedback: Vec<FeedbackRowView>,
    branch: String,
) -> RunDetail {
    let factory = if fm.factory.is_empty() {
        state.map(|s| s.factory.clone()).unwrap_or_default()
    } else {
        fm.factory.clone()
    };
    let title = fm
        .title
        .clone()
        .filter(|t| !t.is_empty())
        .or_else(|| frontmatter::first_heading(body))
        .unwrap_or_else(|| slug.to_string());
    let (stations, active_phase) = match state {
        Some(state) => {
            let views = station_views(state, &factory);
            let active = views
                .iter()
                .find(|v| v.status == StationStatus::Current)
                .and_then(|v| v.phase);
            (views, active)
        }
        None => (Vec::new(), None),
    };
    units.sort_by(|a, b| a.depends_on.len().cmp(&b.depends_on.len()).then(a.slug.cmp(&b.slug)));
    RunDetail {
        slug: slug.to_string(),
        title,
        factory,
        mode: format!("{:?}", fm.mode).to_lowercase(),
        status: status_str(fm.status).to_string(),
        source_ref: branch,
        started_at: fm.started_at.clone(),
        body_html: crate::content::render_markdown(body),
        stations,
        active_phase,
        units,
        feedback,
    }
}

/// Build a detail from a `path -> contents` map (the GitLab shape) at `branch`.
fn assemble_detail_from_map(
    slug: &str,
    map: &BTreeMap<String, String>,
    branch: String,
) -> Result<RunDetail, BrowseError> {
    let run_md = map
        .get(&format!(".darkrun/{slug}/run.md"))
        .cloned()
        .ok_or(BrowseError::NoWorkspace)?;
    let (fm, body) = frontmatter::parse::<RunFrontmatter>(&run_md)
        .map_err(|e| BrowseError::Parse(e.to_string()))?;
    let state = map
        .get(&format!(".darkrun/{slug}/state.json"))
        .and_then(|t| serde_json::from_str::<RunState>(t).ok());
    let unit_prefix = format!(".darkrun/{slug}/units/");
    let fb_prefix = format!(".darkrun/{slug}/feedback/");
    let mut units = Vec::new();
    let mut feedback = Vec::new();
    for (path, content) in map {
        if path.starts_with(&unit_prefix) && path.ends_with(".md") {
            if let Ok((ufm, ubody)) = frontmatter::parse::<UnitFrontmatter>(content) {
                units.push(unit_row(&file_stem(path), &ufm, &ubody));
            }
        } else if path.starts_with(&fb_prefix) && path.ends_with(".md") {
            if let Ok((ffm, _)) = frontmatter::parse::<FeedbackFm>(content) {
                feedback.push(feedback_row(path, ffm));
            }
        }
    }
    Ok(assemble_detail(slug, &fm, &body, state.as_ref(), units, feedback, branch))
}

// ---------------------------------------------------------------------------
// GitHub GraphQL.
// ---------------------------------------------------------------------------

/// The GitHub GraphQL endpoint.
const GH_GRAPHQL: &str = "https://api.github.com/graphql";

/// A resolved `object(expression:)` — a Blob (`text`) or a Tree (`entries`).
#[derive(Debug, Deserialize)]
struct GhObject {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    entries: Option<Vec<GhEntry>>,
}

#[derive(Debug, Deserialize)]
struct GhEntry {
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    object: Option<GhObject>,
}

#[derive(Debug, Deserialize)]
struct GhNamed {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GhListData {
    repository: GhListRepo,
}

#[derive(Debug, Deserialize)]
struct GhListRepo {
    refs: GhRefNodes,
    #[serde(rename = "darkrunDir")]
    darkrun_dir: Option<GhObject>,
}

#[derive(Debug, Deserialize)]
struct GhRefNodes {
    nodes: Vec<GhNamed>,
}

#[derive(Debug, Deserialize)]
struct GhBlobsData {
    repository: BTreeMap<String, Option<GhObject>>,
}

#[derive(Debug, Deserialize)]
struct GhDetailData {
    repository: GhDetailRepo,
}

#[derive(Debug, Deserialize)]
struct GhDetailRepo {
    runmd: Option<GhObject>,
    state: Option<GhObject>,
    units: Option<GhObject>,
    feedback: Option<GhObject>,
}

/// Query A: run-main branches + the default `.darkrun` tree, in one request.
fn gh_list_query() -> &'static str {
    "query($owner:String!,$name:String!){repository(owner:$owner,name:$name){\
     refs(refPrefix:\"refs/heads/darkrun/\",first:100){nodes{name}} \
     darkrunDir:object(expression:\"HEAD:.darkrun\"){...on Tree{entries{name type}}}}}"
}

/// Query B: every run's `run.md` + `state.json` in one aliased request.
fn gh_blobs_query(runs: &[(String, String)]) -> String {
    let mut fields = String::new();
    for (i, (slug, gref)) in runs.iter().enumerate() {
        fields.push_str(&format!(
            "r{i}_run:object(expression:{}){{...on Blob{{text}}}} \
             r{i}_state:object(expression:{}){{...on Blob{{text}}}} ",
            gql_str(&format!("{gref}:.darkrun/{slug}/run.md")),
            gql_str(&format!("{gref}:.darkrun/{slug}/state.json")),
        ));
    }
    format!("query($owner:String!,$name:String!){{repository(owner:$owner,name:$name){{{fields}}}}}")
}

/// The whole-run detail query at `gref` (blobs inline for units + feedback).
fn gh_detail_query(slug: &str, gref: &str) -> String {
    format!(
        "query($owner:String!,$name:String!){{repository(owner:$owner,name:$name){{\
         runmd:object(expression:{run}){{...on Blob{{text}}}} \
         state:object(expression:{state}){{...on Blob{{text}}}} \
         units:object(expression:{units}){{...on Tree{{entries{{name type object{{...on Blob{{text}}}}}}}}}} \
         feedback:object(expression:{feedback}){{...on Tree{{entries{{name type object{{...on Blob{{text}}}}}}}}}}\
         }}}}",
        run = gql_str(&format!("{gref}:.darkrun/{slug}/run.md")),
        state = gql_str(&format!("{gref}:.darkrun/{slug}/state.json")),
        units = gql_str(&format!("{gref}:.darkrun/{slug}/units")),
        feedback = gql_str(&format!("{gref}:.darkrun/{slug}/feedback")),
    )
}

/// Fetch the run list via GitHub GraphQL (two batched queries).
async fn gh_run_list(repo: &RepoRef, token: &str) -> Result<Vec<RunCardData>, BrowseError> {
    let vars = serde_json::json!({ "owner": repo.owner, "name": repo.repo });
    let a: GhListData = graphql(GH_GRAPHQL, Some(token), gh_list_query(), vars.clone()).await?;

    // `refPrefix` strips `refs/heads/darkrun/`, so node names are tails
    // (`<slug>/main`, `<slug>/<station>`, …). Derive one read ref per run slug.
    let mut runs = runs_from_branch_tails(a.repository.refs.nodes.iter().map(|n| n.name.clone()));
    if let Some(dir) = a.repository.darkrun_dir {
        for e in dir.entries.unwrap_or_default() {
            if e.kind == "tree" && e.name != "knowledge" {
                runs.entry(e.name).or_insert_with(|| "HEAD".to_string());
            }
        }
    }
    if runs.is_empty() {
        return Err(BrowseError::NoWorkspace);
    }

    let ordered: Vec<(String, String)> = runs.into_iter().collect();
    let b: GhBlobsData = graphql(GH_GRAPHQL, Some(token), &gh_blobs_query(&ordered), vars).await?;
    let cards: Vec<RunCardData> = ordered
        .iter()
        .enumerate()
        .filter_map(|(i, (slug, _))| {
            let run = b.repository.get(&format!("r{i}_run"))?.as_ref()?.text.clone()?;
            let state_txt = b
                .repository
                .get(&format!("r{i}_state"))
                .and_then(|o| o.as_ref())
                .and_then(|o| o.text.clone());
            let (fm, body) = frontmatter::parse::<RunFrontmatter>(&run).ok()?;
            let state = state_txt.and_then(|t| serde_json::from_str::<RunState>(&t).ok());
            Some(run_card(slug, &fm, &body, state.as_ref()))
        })
        .collect();
    if cards.is_empty() {
        return Err(BrowseError::NoWorkspace);
    }
    Ok(cards)
}

/// Resolve query: the run's own branches + run-main `state.json` (to walk the
/// cursor and learn the active station), in one request.
fn gh_resolve_query(slug: &str) -> String {
    format!(
        "query($owner:String!,$name:String!){{repository(owner:$owner,name:$name){{\
         refs(refPrefix:{prefix},first:100){{nodes{{name}}}} \
         state:object(expression:{state}){{...on Blob{{text}}}}}}}}",
        prefix = gql_str(&format!("refs/heads/{BRANCH_PREFIX}/{slug}/")),
        state = gql_str(&format!("{}:.darkrun/{slug}/state.json", run_main_branch(slug))),
    )
}

#[derive(Debug, Deserialize)]
struct GhResolveData {
    repository: GhResolveRepo,
}

#[derive(Debug, Deserialize)]
struct GhResolveRepo {
    refs: GhRefNodes,
    state: Option<GhObject>,
}

/// Fetch one run's full detail. First walks the cursor (run-main `state.json`)
/// to find the active station, then reads the detail from the **right branch**:
/// the current station's branch (live, not-yet-landed work) when it exists, then
/// run-main (completed stations), then the default branch (sealed run). The
/// chosen ref's tree already carries the completed stations from its run-main
/// base, so a single read shows completed-off-main + current-off-its-branch.
async fn gh_run_detail(repo: &RepoRef, slug: &str, token: &str) -> Result<RunDetail, BrowseError> {
    let vars = serde_json::json!({ "owner": repo.owner, "name": repo.repo });

    // Walk the cursor: the run's branches + the active station from run-main.
    let res: GhResolveData =
        graphql(GH_GRAPHQL, Some(token), &gh_resolve_query(slug), vars.clone()).await?;
    // `refPrefix` strips `refs/heads/darkrun/<slug>/`, so names are bare stations
    // (`main`, `frame`, …).
    let branch_stations: Vec<String> = res.repository.refs.nodes.iter().map(|n| n.name.clone()).collect();
    let active = res
        .repository
        .state
        .and_then(|o| o.text)
        .and_then(|t| serde_json::from_str::<RunState>(&t).ok())
        .map(|s| s.active_station)
        .filter(|s| !s.is_empty());

    // Read-from order: current station branch → run-main → other station
    // branches (covers a brand-new run with no run-main) → default.
    let mut candidates: Vec<(String, String)> = Vec::new();
    if let Some(st) = active.as_ref().filter(|st| branch_stations.iter().any(|n| n == *st)) {
        let b = station_branch(slug, st);
        candidates.push((b.clone(), b));
    }
    if branch_stations.iter().any(|n| n == "main") {
        candidates.push((run_main_branch(slug), run_main_branch(slug)));
    }
    for st in &branch_stations {
        if st != "main" {
            let b = station_branch(slug, st);
            if !candidates.iter().any(|(g, _)| g == &b) {
                candidates.push((b.clone(), b));
            }
        }
    }
    candidates.push(("HEAD".to_string(), "default branch".to_string()));

    for (gref, branch_label) in candidates {
        let d: GhDetailData =
            graphql(GH_GRAPHQL, Some(token), &gh_detail_query(slug, &gref), vars.clone()).await?;
        let Some(run_md) = d.repository.runmd.and_then(|o| o.text) else {
            continue;
        };
        let (fm, body) = frontmatter::parse::<RunFrontmatter>(&run_md)
            .map_err(|e| BrowseError::Parse(e.to_string()))?;
        let state = d
            .repository
            .state
            .and_then(|o| o.text)
            .and_then(|t| serde_json::from_str::<RunState>(&t).ok());
        let units = gh_entries_to_units(d.repository.units);
        let feedback = gh_entries_to_feedback(d.repository.feedback);
        return Ok(assemble_detail(slug, &fm, &body, state.as_ref(), units, feedback, branch_label));
    }
    Err(BrowseError::NoWorkspace)
}

/// Project a GraphQL `units` tree's inline blobs into unit rows.
fn gh_entries_to_units(tree: Option<GhObject>) -> Vec<UnitRowView> {
    tree.and_then(|t| t.entries)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.kind == "blob" && e.name.ends_with(".md"))
        .filter_map(|e| {
            let text = e.object.and_then(|o| o.text)?;
            let slug = e.name.strip_suffix(".md").unwrap_or(&e.name).to_string();
            let (fm, body) = frontmatter::parse::<UnitFrontmatter>(&text).ok()?;
            Some(unit_row(&slug, &fm, &body))
        })
        .collect()
}

/// Project a GraphQL `feedback` tree's inline blobs into feedback rows.
fn gh_entries_to_feedback(tree: Option<GhObject>) -> Vec<FeedbackRowView> {
    tree.and_then(|t| t.entries)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.kind == "blob" && e.name.ends_with(".md"))
        .filter_map(|e| {
            let text = e.object.and_then(|o| o.text)?;
            let (fm, _) = frontmatter::parse::<FeedbackFm>(&text).ok()?;
            Some(feedback_row(&e.name, fm))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// GitLab GraphQL.
// ---------------------------------------------------------------------------

/// Branch-name pages to walk at most (100 each) — a sane cap on a repo's branch
/// count while discovering run-main branches.
const GL_BRANCH_MAX_PAGES: u32 = 6;

#[derive(Debug, Deserialize)]
struct GlData {
    project: Option<GlProject>,
}

#[derive(Debug, Deserialize)]
struct GlProject {
    repository: Option<GlRepo>,
}

#[derive(Debug, Deserialize)]
struct GlRepo {
    #[serde(rename = "rootRef", default)]
    root_ref: Option<String>,
    #[serde(rename = "branchNames", default)]
    branch_names: Option<Vec<String>>,
    #[serde(default)]
    tree: Option<GlTree>,
}

#[derive(Debug, Deserialize)]
struct GlTree {
    #[serde(default)]
    trees: Option<GlNamedNodes>,
    #[serde(default)]
    blobs: Option<GlPathNodes>,
}

#[derive(Debug, Deserialize)]
struct GlNamedNodes {
    #[serde(default)]
    nodes: Vec<GlNamed>,
}

#[derive(Debug, Deserialize)]
struct GlNamed {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GlPathNodes {
    #[serde(default)]
    nodes: Vec<GlPathNode>,
}

#[derive(Debug, Deserialize)]
struct GlPathNode {
    path: String,
}

#[derive(Debug, Deserialize)]
struct GlBlobsData {
    project: Option<GlBlobsProject>,
}

#[derive(Debug, Deserialize)]
struct GlBlobsProject {
    #[serde(default)]
    repository: Option<BTreeMap<String, GlBlobNodes>>,
}

#[derive(Debug, Deserialize)]
struct GlBlobNodes {
    #[serde(default)]
    nodes: Vec<GlBlobNode>,
}

#[derive(Debug, Deserialize)]
struct GlBlobNode {
    path: String,
    #[serde(rename = "rawTextBlob", default)]
    raw_text_blob: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GlDetailBlobsData {
    project: Option<GlDetailBlobsProject>,
}

#[derive(Debug, Deserialize)]
struct GlDetailBlobsProject {
    repository: Option<GlDetailBlobsRepo>,
}

#[derive(Debug, Deserialize)]
struct GlDetailBlobsRepo {
    #[serde(default)]
    blobs: Option<GlBlobNodes>,
}

/// The GitLab GraphQL endpoint for this host.
fn gl_graphql_url(repo: &RepoRef) -> String {
    format!("https://{}/api/graphql", repo.host)
}

/// Discovery query: default branch + the default `.darkrun` tree (sealed runs).
fn gl_disc_query() -> &'static str {
    "query($path:ID!){project(fullPath:$path){repository{\
     rootRef tree(ref:\"HEAD\",path:\".darkrun\"){trees{nodes{name}}}}}}"
}

/// A page of branch names (search-all; filtered to run-main branches client-side).
fn gl_branch_names_query() -> &'static str {
    "query($path:ID!,$offset:Int!){project(fullPath:$path){repository{\
     branchNames(searchPattern:\"*\",offset:$offset,limit:100)}}}"
}

/// Resolve query: default branch + the run's own branches (`darkrun/<slug>/*`) +
/// run-main `state.json` — enough to walk the cursor and pick the live ref.
fn gl_resolve_query(slug: &str) -> String {
    let mut q = String::from(
        "query($path:ID!){project(fullPath:$path){repository{rootRef branchNames(searchPattern:",
    );
    q.push_str(&gql_str(&format!("{BRANCH_PREFIX}/{slug}/*")));
    q.push_str(",offset:0,limit:100) mainState:blobs(ref:");
    q.push_str(&gql_str(&run_main_branch(slug)));
    q.push_str(",paths:[");
    q.push_str(&gql_str(&format!(".darkrun/{slug}/state.json")));
    q.push_str("]){nodes{path rawTextBlob}}}}}");
    q
}

#[derive(Debug, Deserialize)]
struct GlResolveData {
    project: Option<GlResolveProject>,
}

#[derive(Debug, Deserialize)]
struct GlResolveProject {
    repository: Option<GlResolveRepo>,
}

#[derive(Debug, Deserialize)]
struct GlResolveRepo {
    #[serde(rename = "rootRef", default)]
    root_ref: Option<String>,
    #[serde(rename = "branchNames", default)]
    branch_names: Option<Vec<String>>,
    #[serde(rename = "mainState", default)]
    main_state: Option<GlBlobNodes>,
}

/// Recursive tree of a run's `.darkrun/<slug>` at `gref` → blob paths.
fn gl_tree_query(slug: &str, gref: &str) -> String {
    let mut q = String::from("query($path:ID!){project(fullPath:$path){repository{tree(ref:");
    q.push_str(&gql_str(gref));
    q.push_str(",path:");
    q.push_str(&gql_str(&format!(".darkrun/{slug}")));
    q.push_str(",recursive:true){blobs{nodes{path}}}}}}");
    q
}

/// Aliased blobs: every run's `run.md` + `state.json` at its own ref, one query.
fn gl_blobs_query(runs: &[(String, String)]) -> String {
    let mut fields = String::new();
    for (i, (slug, gref)) in runs.iter().enumerate() {
        let paths = format!(
            "[{},{}]",
            gql_str(&format!(".darkrun/{slug}/run.md")),
            gql_str(&format!(".darkrun/{slug}/state.json")),
        );
        fields.push_str(&format!(
            "r{i}:blobs(ref:{},paths:{}){{nodes{{path rawTextBlob}}}} ",
            gql_str(gref),
            paths,
        ));
    }
    format!("query($path:ID!){{project(fullPath:$path){{repository{{ {fields} }}}}}}")
}

/// All of a run's blob contents at `gref`, by path, in one query.
fn gl_detail_blobs_query(gref: &str, paths: &[String]) -> String {
    let arr = format!(
        "[{}]",
        paths.iter().map(|p| gql_str(p)).collect::<Vec<_>>().join(","),
    );
    let mut q = String::from("query($path:ID!){project(fullPath:$path){repository{blobs(ref:");
    q.push_str(&gql_str(gref));
    q.push_str(",paths:");
    q.push_str(&arr);
    q.push_str("){nodes{path rawTextBlob}}}}}");
    q
}

/// Walk `branchNames` pages and return every branch name (filtered later).
async fn gl_all_branch_names(
    url: &str,
    token: Option<&str>,
    path: &str,
) -> Result<Vec<String>, BrowseError> {
    let mut all = Vec::new();
    for page in 0..GL_BRANCH_MAX_PAGES {
        let vars = serde_json::json!({ "path": path, "offset": page * 100 });
        let d: GlData = graphql(url, token, gl_branch_names_query(), vars).await?;
        let names = d
            .project
            .and_then(|p| p.repository)
            .and_then(|r| r.branch_names)
            .unwrap_or_default();
        let n = names.len();
        all.extend(names);
        if n < 100 {
            break;
        }
    }
    Ok(all)
}

/// Fetch the run list via GitLab GraphQL.
async fn gl_run_list(repo: &RepoRef, token: Option<&str>) -> Result<Vec<RunCardData>, BrowseError> {
    let url = gl_graphql_url(repo);
    let path = repo.gl_full_path();
    let vars = serde_json::json!({ "path": path });

    let disc: GlData = graphql(&url, token, gl_disc_query(), vars).await?;
    let repo_node = disc.project.and_then(|p| p.repository);
    let default_ref = repo_node
        .as_ref()
        .and_then(|r| r.root_ref.clone())
        .unwrap_or_else(|| "HEAD".to_string());
    let sealed: Vec<String> = repo_node
        .as_ref()
        .and_then(|r| r.tree.as_ref())
        .and_then(|t| t.trees.as_ref())
        .map(|n| {
            n.nodes
                .iter()
                .map(|x| x.name.clone())
                .filter(|name| name != "knowledge")
                .collect()
        })
        .unwrap_or_default();

    // Branch names come back fully-qualified (`darkrun/<slug>/…`); strip the
    // prefix to tails and choose a read ref per run slug (run-main, else a
    // station branch — so brand-new runs with no run-main are found).
    let prefix = format!("{BRANCH_PREFIX}/");
    let tails = gl_all_branch_names(&url, token, &path)
        .await?
        .into_iter()
        .filter_map(|n| n.strip_prefix(&prefix).map(|s| s.to_string()));
    let mut runs = runs_from_branch_tails(tails);
    for slug in sealed {
        runs.entry(slug).or_insert_with(|| default_ref.clone());
    }
    if runs.is_empty() {
        return Err(BrowseError::NoWorkspace);
    }

    let ordered: Vec<(String, String)> = runs.into_iter().collect();
    let b: GlBlobsData = graphql(
        &url,
        token,
        &gl_blobs_query(&ordered),
        serde_json::json!({ "path": path }),
    )
    .await?;
    let repo_map = b.project.and_then(|p| p.repository).unwrap_or_default();
    let cards: Vec<RunCardData> = ordered
        .iter()
        .enumerate()
        .filter_map(|(i, (slug, _))| {
            let nodes = &repo_map.get(&format!("r{i}"))?.nodes;
            let mut runmd = None;
            let mut state_txt = None;
            for node in nodes {
                if node.path.ends_with("/run.md") {
                    runmd = node.raw_text_blob.clone();
                } else if node.path.ends_with("/state.json") {
                    state_txt = node.raw_text_blob.clone();
                }
            }
            let (fm, body) = frontmatter::parse::<RunFrontmatter>(&runmd?).ok()?;
            let state = state_txt.and_then(|t| serde_json::from_str::<RunState>(&t).ok());
            Some(run_card(slug, &fm, &body, state.as_ref()))
        })
        .collect();
    if cards.is_empty() {
        return Err(BrowseError::NoWorkspace);
    }
    Ok(cards)
}

/// Fetch one run's full detail via GitLab GraphQL (resolve ref → tree → blobs).
async fn gl_run_detail(
    repo: &RepoRef,
    slug: &str,
    token: Option<&str>,
) -> Result<RunDetail, BrowseError> {
    let url = gl_graphql_url(repo);
    let path = repo.gl_full_path();

    // Walk the cursor: the run's branches + the active station from run-main,
    // then pick the live ref — the current station's branch (its in-progress
    // work) when present, else run-main (completed stations), else a station
    // branch (a brand-new run with no run-main yet), else the default branch.
    let r: GlResolveData =
        graphql(&url, token, &gl_resolve_query(slug), serde_json::json!({ "path": path })).await?;
    let rn = r.project.and_then(|p| p.repository);
    let default_ref = rn
        .as_ref()
        .and_then(|x| x.root_ref.clone())
        .unwrap_or_else(|| "HEAD".to_string());
    let branches = rn.as_ref().and_then(|x| x.branch_names.clone()).unwrap_or_default();
    let active = rn
        .as_ref()
        .and_then(|x| x.main_state.as_ref())
        .and_then(|b| b.nodes.iter().find_map(|n| n.raw_text_blob.clone()))
        .and_then(|t| serde_json::from_str::<RunState>(&t).ok())
        .map(|s| s.active_station)
        .filter(|s| !s.is_empty());

    let main_branch = run_main_branch(slug);
    let station_b = active.as_ref().map(|st| station_branch(slug, st));
    let (gref, branch_label) = if let Some(b) = station_b.filter(|b| branches.contains(b)) {
        (b.clone(), b)
    } else if branches.contains(&main_branch) {
        (main_branch.clone(), main_branch)
    } else if let Some(b) = branches.iter().find(|n| *n != &main_branch).cloned() {
        (b.clone(), b)
    } else {
        (default_ref, "default branch".to_string())
    };

    // The run's blob paths, then their contents.
    let t: GlData = graphql(
        &url,
        token,
        &gl_tree_query(slug, &gref),
        serde_json::json!({ "path": path }),
    )
    .await?;
    let paths: Vec<String> = t
        .project
        .and_then(|p| p.repository)
        .and_then(|r| r.tree)
        .and_then(|t| t.blobs)
        .map(|b| b.nodes.into_iter().map(|n| n.path).collect())
        .unwrap_or_default();
    if paths.is_empty() {
        return Err(BrowseError::NoWorkspace);
    }

    let b: GlDetailBlobsData = graphql(
        &url,
        token,
        &gl_detail_blobs_query(&gref, &paths),
        serde_json::json!({ "path": path }),
    )
    .await?;
    let nodes = b
        .project
        .and_then(|p| p.repository)
        .and_then(|r| r.blobs)
        .map(|b| b.nodes)
        .unwrap_or_default();
    let map: BTreeMap<String, String> = nodes
        .into_iter()
        .filter_map(|n| n.raw_text_blob.map(|t| (n.path, t)))
        .collect();

    assemble_detail_from_map(slug, &map, branch_label)
}

// ---------------------------------------------------------------------------
// Public dispatchers.
// ---------------------------------------------------------------------------

/// Fetch a repository's run list over GraphQL. GitHub needs a `token` (no
/// anonymous GraphQL); GitLab reads anonymously (a token unlocks private).
pub async fn fetch_run_list(
    repo: &RepoRef,
    token: Option<&str>,
) -> Result<Vec<RunCardData>, BrowseError> {
    match repo.provider()? {
        Provider::GitHub => match token {
            Some(t) => gh_run_list(repo, t).await,
            None => Err(BrowseError::NeedsAuth("GitHub")),
        },
        Provider::GitLab => gl_run_list(repo, token).await,
    }
}

/// Fetch a single run's full detail over GraphQL.
pub async fn fetch_run_detail(
    repo: &RepoRef,
    slug: &str,
    token: Option<&str>,
) -> Result<RunDetail, BrowseError> {
    match repo.provider()? {
        Provider::GitHub => match token {
            Some(t) => gh_run_detail(repo, slug, t).await,
            None => Err(BrowseError::NeedsAuth("GitHub")),
        },
        Provider::GitLab => gl_run_detail(repo, slug, token).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn provider_detection_matches_the_vcs_crate() {
        assert_eq!(Provider::from_host("github.com"), Some(Provider::GitHub));
        assert_eq!(Provider::from_host("gitlab.com"), Some(Provider::GitLab));
        assert_eq!(Provider::from_host("gitlab.example.com"), Some(Provider::GitLab));
        assert_eq!(Provider::from_host("bitbucket.org"), None);
    }

    #[test]
    fn run_main_branch_uses_the_engine_namespace() {
        assert_eq!(run_main_branch("my-run"), "darkrun/my-run/main");
    }

    #[test]
    fn runs_from_branch_tails_finds_station_only_runs() {
        // A brand-new run (only a station branch, no run-main) is still found;
        // a run with run-main prefers it over its station branches.
        let tails = vec![
            "darkrun-sim/frame".to_string(),
            "darkrun-sim/units/frame/u1".to_string(), // deeper branch — ignored for the ref
            "shipped/main".to_string(),
            "shipped/build".to_string(),
            "knowledge".to_string(), // not a run branch (no `/`)
        ];
        let runs = runs_from_branch_tails(tails);
        assert_eq!(runs.get("darkrun-sim").map(String::as_str), Some("darkrun/darkrun-sim/frame"));
        assert_eq!(runs.get("shipped").map(String::as_str), Some("darkrun/shipped/main"));
        assert_eq!(runs.len(), 2);
    }

    #[test]
    fn gl_full_path_is_owner_slash_repo() {
        let repo = RepoRef { host: "gitlab.com".into(), owner: "grp".into(), repo: "sub/proj".into() };
        assert_eq!(repo.gl_full_path(), "grp/sub/proj");
        assert_eq!(gl_graphql_url(&repo), "https://gitlab.com/api/graphql");
    }

    #[test]
    fn target_parses_repo_and_run() {
        assert_eq!(
            Target::parse(&seg(&["github.com", "darkrun-ai", "darkrun"])).unwrap(),
            Target::RunList(RepoRef {
                host: "github.com".into(),
                owner: "darkrun-ai".into(),
                repo: "darkrun".into(),
            })
        );
        match Target::parse(&seg(&["gitlab.com", "grp", "sub", "proj", "run", "x"])).unwrap() {
            Target::Run(repo, slug) => {
                assert_eq!(repo.repo, "sub/proj");
                assert_eq!(slug, "x");
            }
            _ => panic!("expected a run"),
        }
        assert!(Target::parse(&seg(&["github.com", "owner"])).is_none());
    }

    #[test]
    fn run_rest_round_trips() {
        let repo = RepoRef { host: "github.com".into(), owner: "o".into(), repo: "r".into() };
        assert_eq!(repo.run_path("slug"), "/browse/github.com/o/r/run/slug");
        assert_eq!(repo.run_rest("slug"), seg(&["github.com", "o", "r", "run", "slug"]));
    }

    #[test]
    fn normalize_accepts_github_and_gitlab_forms() {
        assert_eq!(
            normalize_repo_input("https://github.com/darkrun-ai/darkrun.git"),
            Some(seg(&["github.com", "darkrun-ai", "darkrun"]))
        );
        assert_eq!(
            normalize_repo_input("git@github.com:darkrun-ai/darkrun.git"),
            Some(seg(&["github.com", "darkrun-ai", "darkrun"]))
        );
        assert_eq!(
            normalize_repo_input("https://gitlab.com/grp/sub/proj"),
            Some(seg(&["gitlab.com", "grp", "sub", "proj"]))
        );
        assert_eq!(normalize_repo_input("owner/repo"), None);
    }

    // ── query builders ───────────────────────────────────────────────────

    #[test]
    fn gql_str_encodes_a_graphql_string_literal() {
        assert_eq!(gql_str("darkrun/r/main:.darkrun/r/run.md"), "\"darkrun/r/main:.darkrun/r/run.md\"");
        assert_eq!(gql_str("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn gh_blobs_query_aliases_every_run_at_its_ref() {
        let runs = vec![
            ("alpha".to_string(), "darkrun/alpha/main".to_string()),
            ("beta".to_string(), "HEAD".to_string()),
        ];
        let q = gh_blobs_query(&runs);
        assert!(q.contains("r0_run:object(expression:\"darkrun/alpha/main:.darkrun/alpha/run.md\")"));
        assert!(q.contains("r1_run:object(expression:\"HEAD:.darkrun/beta/run.md\")"));
    }

    #[test]
    fn gh_detail_query_pulls_unit_and_feedback_blobs_inline() {
        let q = gh_detail_query("alpha", "darkrun/alpha/main");
        assert!(q.contains("units:object(expression:\"darkrun/alpha/main:.darkrun/alpha/units\")"));
        assert!(q.contains("entries{name type object{...on Blob{text}}}"));
    }

    #[test]
    fn gl_blobs_query_aliases_each_run_with_a_paths_array() {
        let runs = vec![("alpha".to_string(), "darkrun/alpha/main".to_string())];
        let q = gl_blobs_query(&runs);
        assert!(q.contains("r0:blobs(ref:\"darkrun/alpha/main\""));
        assert!(q.contains("\".darkrun/alpha/run.md\""));
        assert!(q.contains("\".darkrun/alpha/state.json\""));
        assert!(q.contains("nodes{path rawTextBlob}"));
        // Balanced braces (a miscount would unbalance the query).
        assert_eq!(q.matches('{').count(), q.matches('}').count());
    }

    #[test]
    fn gl_tree_and_detail_queries_are_brace_balanced() {
        let t = gl_tree_query("alpha", "darkrun/alpha/main");
        assert!(t.contains("tree(ref:\"darkrun/alpha/main\",path:\".darkrun/alpha\",recursive:true)"));
        assert_eq!(t.matches('{').count(), t.matches('}').count());

        let d = gl_detail_blobs_query("HEAD", &[".darkrun/a/run.md".to_string()]);
        assert!(d.contains("blobs(ref:\"HEAD\",paths:[\".darkrun/a/run.md\"])"));
        assert_eq!(d.matches('{').count(), d.matches('}').count());

        let r = gl_resolve_query("alpha");
        assert!(r.contains("branchNames(searchPattern:\"darkrun/alpha/*\",offset:0,limit:100)"));
        assert!(r.contains("mainState:blobs(ref:\"darkrun/alpha/main\""));
        assert_eq!(r.matches('{').count(), r.matches('}').count());
    }

    #[test]
    fn gh_entries_to_units_parses_inline_blob_text() {
        let tree = Some(GhObject {
            text: None,
            entries: Some(vec![
                GhEntry {
                    name: "u1.md".to_string(),
                    kind: "blob".to_string(),
                    object: Some(GhObject {
                        text: Some(
                            "---\nstatus: completed\nunit_type: feature\ndepends_on: []\n---\n# One\n"
                                .to_string(),
                        ),
                        entries: None,
                    }),
                },
                GhEntry { name: "sub".to_string(), kind: "tree".to_string(), object: None },
            ]),
        });
        let units = gh_entries_to_units(tree);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].slug, "u1");
        assert_eq!(units[0].status, "completed");
    }

    #[test]
    fn assemble_detail_from_map_builds_units_and_feedback() {
        let mut map = BTreeMap::new();
        map.insert(
            ".darkrun/alpha/run.md".to_string(),
            "---\nfactory: software\nmode: solo\nstatus: active\nactive_station: frame\n---\n# Alpha\n".to_string(),
        );
        map.insert(
            ".darkrun/alpha/units/u1.md".to_string(),
            "---\nstatus: completed\nunit_type: feature\ndepends_on: []\n---\n# U1\n".to_string(),
        );
        map.insert(
            ".darkrun/alpha/feedback/FB-01.md".to_string(),
            "---\nfeedback_id: FB-01\ntitle: A nit\nstatus: open\n---\nbody\n".to_string(),
        );
        let d = assemble_detail_from_map("alpha", &map, "darkrun/alpha/main".to_string()).unwrap();
        assert_eq!(d.title, "Alpha");
        assert_eq!(d.units.len(), 1);
        assert_eq!(d.units[0].slug, "u1");
        assert_eq!(d.feedback.len(), 1);
        assert_eq!(d.feedback[0].id, "FB-01");
        assert_eq!(d.source_ref, "darkrun/alpha/main");
    }
}
