//! The darkrun **GitHub App** — the backend-mediated, installation-scoped view
//! of a user's repositories and their committed runs.
//!
//! The OAuth-token endpoints (`repos.rs`/`sessions.rs`) answer "what can THIS
//! browser's ephemeral provider token reach?". That token is minted at sign-in
//! and dies with the tab. The web app wants a *persistent* workspace: sign in
//! once, and on every later load see your repos and their darkrun runs without
//! re-authorizing a provider. The durable key is the user's **Firebase
//! identity**, not a provider access token — so the data must come from
//! somewhere the server can reach with only that identity.
//!
//! That somewhere is the darkrun **GitHub App installation**. The App holds its
//! own RSA key; it self-signs a short-lived App JWT (RS256 — the same signing
//! pattern [`crate::gcp_auth`] uses for the Google SA assertion), lists its
//! installations, mints a per-installation token, and lists that installation's
//! repositories. Given the GitHub identity carried in the verified Firebase
//! token (numeric user id, see [`crate::firebase_auth`]), the server selects the
//! installation(s) whose account is that user, plus any **organization**
//! installation the user is a *verified member* of, and returns their repos —
//! each already carrying its `.darkrun/` runs, read the same committed-tree way
//! `sessions.rs` reads them.
//!
//! # Organization coverage requirements
//!
//! The darkrun App is typically installed on an **organization** (e.g.
//! `darkrun-ai`), not on the individual's personal account. An org installation's
//! repos are only included for a user when **all** of the following hold:
//!
//! 1. The darkrun GitHub App is **installed on that organization**.
//! 2. The App has the **`Members: read`** organization permission (the
//!    "Organization permissions → Members: Read-only" grant). Without it, the
//!    `GET /orgs/{org}/members` call returns 403 and the org is skipped.
//! 3. The signed-in user is a **verified member** of the org — matched by their
//!    numeric GitHub user id against the org's member list.
//!
//! Membership is proven by listing the org's members through the **org
//! installation token** and matching the caller's numeric id (the numeric id is
//! the stable key; it sidesteps a fragile id→login resolution step). If that
//! check errors for any reason (missing `Members: read`, a 403, a timeout, a
//! transport failure), the org is **skipped, never included** — we fail safe and
//! log a `tracing::warn`, so a permission gap can never leak an org's repos.
//!
//! Everything rides the injectable [`HttpTransport`](darkrun_vcs::HttpTransport)
//! seam, so the pure parts — JWT claims, member-id parsing, repo/run
//! normalization — are unit-tested fully offline; only the signing + network
//! calls need real credentials.
//!
//! Absent config (`GITHUB_APP_ID` / `GITHUB_APP_PRIVATE_KEY` unset) is a
//! *disabled feature*, not a crash: [`GitHubApp::from_env`] returns `None` and
//! the workspace endpoints answer with a clear "not configured" error.

use darkrun_vcs::{HttpRequest, Provider};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::sessions::{parse_sessions, tree_truncated, tree_url, DiscoveredSession};

/// A user-agent string. GitHub rejects API requests without one.
const USER_AGENT: &str = "darkrun-web";

/// The App JWT's lifetime, in seconds. GitHub caps an App JWT at 10 minutes; we
/// use 9 to leave headroom for clock skew on GitHub's side.
const APP_JWT_TTL_SECS: u64 = 540;

/// Backdate the App JWT's `iat` by this many seconds to tolerate our clock
/// running slightly ahead of GitHub's (GitHub rejects a future `iat`).
const APP_JWT_IAT_SKEW_SECS: u64 = 60;

/// Members per page when listing an org's members (`GET /orgs/{org}/members`).
/// GitHub caps this at 100.
const MEMBERS_PER_PAGE: u32 = 100;

/// The most pages of org members we will page through before giving up and
/// treating the caller as "not found on the pages we saw". At
/// [`MEMBERS_PER_PAGE`] members per page this covers 5,000 members; larger orgs
/// are capped and logged (a member past the cap is treated as not-a-member, so
/// the fail-safe direction is to *exclude*, never over-include).
const MEMBERS_MAX_PAGES: u32 = 50;

/// Items per page when listing App installations (`GET /app/installations`) and
/// an installation's repositories (`GET /installation/repositories`). GitHub
/// caps both at 100. Both endpoints paginate with `?page=N`; we walk pages until
/// a page returns fewer than this (the last page).
const APP_LIST_PER_PAGE: u32 = 100;

/// The most pages we walk for the App installation list and for each
/// installation's repository list before stopping. At [`APP_LIST_PER_PAGE`] per
/// page this covers 5,000 items each; the cap is a safety bound so a misbehaving
/// remote (one that never returns a short page) can never spin us forever. If
/// the cap is hit it is logged; items past it are unseen.
const APP_LIST_MAX_PAGES: u32 = 50;

/// The darkrun GitHub App: its numeric App id and RSA private key (PEM).
///
/// Cloneable and cheap to hold in shared state. The key is the App's RUNTIME
/// credential (`GITHUB_APP_PRIVATE_KEY`), never a CI secret.
#[derive(Clone)]
pub struct GitHubApp {
    /// The GitHub App's numeric id (the App JWT `iss`).
    app_id: String,
    /// The App's RSA private key (PEM), used to sign the App JWT.
    private_key: String,
}

/// The App JWT claims. Pure, so the claim set is unit-tested without signing.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct AppJwtClaims {
    /// Issued-at (unix seconds), backdated by [`APP_JWT_IAT_SKEW_SECS`].
    iat: u64,
    /// Expiry (unix seconds).
    exp: u64,
    /// The App id (issuer).
    iss: String,
}

/// One installation as `GET /app/installations` returns it (the subset we use).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Installation {
    /// The installation's numeric id (the path segment for token minting).
    pub id: u64,
    /// The account the App is installed on (a user or an org).
    pub account: InstallationAccount,
}

/// The account an installation belongs to.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct InstallationAccount {
    /// The account login (user or org name).
    #[serde(default)]
    pub login: String,
    /// The account's numeric id.
    #[serde(default)]
    pub id: u64,
    /// `User` or `Organization`.
    #[serde(default, rename = "type")]
    pub account_type: String,
}

/// The GitHub identity extracted from a verified Firebase token: the user's
/// GitHub login and numeric id (see [`crate::firebase_auth`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubIdentity {
    /// The GitHub login (username), if the Firebase identity carried one.
    pub login: Option<String>,
    /// The GitHub numeric user id (the `firebase.identities["github.com"][0]`
    /// value — the stable subject).
    pub user_id: String,
}

/// One repository a workspace lists, with its committed darkrun runs embedded.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkspaceRepo {
    /// The short repository name (e.g. `darkrun`).
    pub name: String,
    /// The owner-qualified path (e.g. `jwaldrip/darkrun`).
    pub full_name: String,
    /// The web URL where the repository lives.
    pub url: String,
    /// The provider the repository is hosted on (always GitHub for the App).
    pub provider: Provider,
    /// The darkrun runs discovered read-only in this repo's `.darkrun/` tree.
    pub runs: Vec<DiscoveredSession>,
}

impl GitHubApp {
    /// Build from explicit parts (used by tests and [`from_env`](Self::from_env)).
    pub fn new(app_id: impl Into<String>, private_key: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            private_key: private_key.into(),
        }
    }

    /// Load the App from `GITHUB_APP_ID` + `GITHUB_APP_PRIVATE_KEY`, or `None`
    /// when either is unset/blank — in which case the feature is disabled and
    /// the workspace endpoints answer with a clear "not configured" error.
    ///
    /// The private key env may carry the PEM with literal `\n` escapes (a common
    /// way to pass a multi-line secret through a single env var); those are
    /// normalized to real newlines so [`EncodingKey::from_rsa_pem`] accepts it.
    #[cfg(not(tarpaulin_include))] // env read
    pub fn from_env() -> Option<Self> {
        let app_id = std::env::var("GITHUB_APP_ID")
            .ok()
            .filter(|v| !v.trim().is_empty())?;
        let private_key = std::env::var("GITHUB_APP_PRIVATE_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty())?;
        Some(Self::new(app_id.trim().to_string(), normalize_pem(&private_key)))
    }

    /// Sign an App JWT valid at `now` (unix seconds). Fails only if the RSA
    /// private key won't load.
    fn app_jwt(&self, now: u64) -> Result<String, String> {
        let claims = app_jwt_claims(&self.app_id, now);
        let key = EncodingKey::from_rsa_pem(self.private_key.as_bytes())
            .map_err(|e| format!("loading GitHub App private key: {e}"))?;
        encode(&Header::new(Algorithm::RS256), &claims, &key)
            .map_err(|e| format!("signing GitHub App JWT: {e}"))
    }

    /// The user-visible list of a signed-in user's repositories (with their
    /// runs), assembled from every installation the user owns or is a verified
    /// member of.
    ///
    /// Orchestration over the synchronous
    /// [`HttpTransport`](darkrun_vcs::HttpTransport) seam, so a caller runs it on
    /// the blocking pool exactly like `repos.rs`/`sessions.rs`.
    ///
    /// Steps: sign an App JWT → list installations → for each installation, mint
    /// its token, then decide inclusion:
    /// - a **User** installation is included when its account IS the caller
    ///   (numeric id, falling back to login);
    /// - an **Organization** installation is included only when the caller is a
    ///   verified member of that org (their numeric id appears in the org's
    ///   member list, read via the org installation token). A membership check
    ///   that errors (missing `Members: read`, 403, timeout, transport failure)
    ///   **skips** the org and logs a `tracing::warn` — never includes it.
    ///
    /// For each included installation, list its repositories and read each repo's
    /// `.darkrun/` runs.
    #[cfg(not(tarpaulin_include))] // network orchestration (parts below are tested)
    pub fn workspace(
        &self,
        transport: &dyn darkrun_vcs::HttpTransport,
        identity: &GitHubIdentity,
        now: u64,
    ) -> Result<Vec<WorkspaceRepo>, String> {
        let jwt = self.app_jwt(now)?;
        let installations = list_installations(transport, &jwt)?;

        let mut repos = Vec::new();
        for inst in &installations {
            // Mint the installation token first: an org's membership check needs
            // it, and it is the same token used to list the repos below.
            let token = installation_token(transport, &jwt, inst.id)?;
            if !installation_covers_user(transport, &token, &inst.account, identity) {
                continue;
            }
            let listed = list_installation_repos(transport, &token)?;
            for mut repo in listed {
                repo.runs = read_runs(transport, &token, &repo.full_name)?;
                repos.push(repo);
            }
        }
        Ok(repos)
    }

    /// Read one run's full committed state from a repo's `.darkrun/<id>/` tree,
    /// via the installation covering that repo.
    ///
    /// Selects the installation by the repo's owner (the first path segment of
    /// `full_name`), mints its token, and reads the run's `state.json` + `run.md`
    /// blobs. `None` from any single blob is tolerated — the detail is assembled
    /// from whatever committed files are present.
    ///
    /// The owning installation is subject to the **same access gate as the
    /// workspace list**: a User installation must BE the caller, and an
    /// Organization installation must be one the caller is a verified member of.
    /// This keeps `/api/run` from serving a repo the caller could not see in the
    /// workspace. A membership check that errors fails safe (access denied).
    #[cfg(not(tarpaulin_include))] // network orchestration (parts below are tested)
    pub fn run_detail(
        &self,
        transport: &dyn darkrun_vcs::HttpTransport,
        identity: &GitHubIdentity,
        full_name: &str,
        run_id: &str,
        now: u64,
    ) -> Result<CommittedRun, String> {
        let jwt = self.app_jwt(now)?;
        let installations = list_installations(transport, &jwt)?;
        let owner = full_name.split('/').next().unwrap_or_default();
        let inst = installation_for_owner(&installations, owner)
            .ok_or_else(|| format!("no darkrun GitHub App installation covers `{owner}`"))?;
        let token = installation_token(transport, &jwt, inst.id)?;

        // Gate exactly as the workspace list does: never serve a repo whose
        // owning installation the caller does not own / is not a verified member
        // of. A membership-check error fails safe (treated as no access).
        if !installation_covers_user(transport, &token, &inst.account, identity) {
            return Err(format!("you do not have access to `{owner}` through darkrun"));
        }

        let state = read_blob(transport, &token, full_name, &state_path(run_id))?;
        let run_md = read_blob(transport, &token, full_name, &run_md_path(run_id))?;
        Ok(build_committed_run(run_id, full_name, state.as_deref(), run_md.as_deref()))
    }
}

/// Normalize a PEM that may carry literal `\n` escapes into real newlines. A key
/// already using real newlines is returned unchanged.
fn normalize_pem(raw: &str) -> String {
    if raw.contains("\\n") && !raw.contains('\n') {
        raw.replace("\\n", "\n")
    } else {
        raw.to_string()
    }
}

/// Build the App JWT claims for `app_id` at `now`. Pure — unit-tested without
/// signing.
fn app_jwt_claims(app_id: &str, now: u64) -> AppJwtClaims {
    AppJwtClaims {
        iat: now.saturating_sub(APP_JWT_IAT_SKEW_SECS),
        exp: now + APP_JWT_TTL_SECS,
        iss: app_id.to_string(),
    }
}

/// Apply the standard App-JWT auth + accept headers. GitHub Apps authenticate
/// as the App with `Authorization: Bearer <app_jwt>`.
fn app_authorize(request: HttpRequest, jwt: &str) -> HttpRequest {
    request
        .header("Authorization", format!("Bearer {jwt}"))
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
}

/// Apply the installation-token auth + accept headers. An installation token
/// authenticates with `Authorization: token <installation_token>`.
fn token_authorize(request: HttpRequest, token: &str) -> HttpRequest {
    request
        .header("Authorization", format!("token {token}"))
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
}

/// List every installation of the App (`GET /app/installations`), walking every
/// `?page=N` until a short page ends the list (bounded by [`APP_LIST_MAX_PAGES`]).
/// The response is a bare array; a user with the App on more than one page of
/// installations is fully covered.
fn list_installations(
    transport: &dyn darkrun_vcs::HttpTransport,
    jwt: &str,
) -> Result<Vec<Installation>, String> {
    let mut installations = Vec::new();
    for page in 1..=APP_LIST_MAX_PAGES {
        let url = format!(
            "{}/app/installations?per_page={}&page={}",
            Provider::GitHub.api_base(),
            APP_LIST_PER_PAGE,
            page,
        );
        let request = app_authorize(HttpRequest::get(url), jwt);
        let response = transport.execute(request).map_err(|e| e.to_string())?;
        if !response.is_success() {
            return Err(format!(
                "listing App installations failed ({})",
                response.status
            ));
        }
        let value: serde_json::Value = response.json().map_err(|e| e.to_string())?;
        // Short-page detection reads the RAW array length (before normalization,
        // which may drop malformed entries) so a full page is never mistaken for
        // the last one.
        let page_len = value.as_array().map(Vec::len).unwrap_or(0);
        installations.extend(parse_installations(&value));
        if page_len < APP_LIST_PER_PAGE as usize {
            break;
        }
        if page == APP_LIST_MAX_PAGES {
            tracing::warn!(
                pages = APP_LIST_MAX_PAGES,
                "App installation list hit the pagination cap; installations past it are unseen",
            );
        }
    }
    Ok(installations)
}

/// Mint an installation access token
/// (`POST /app/installations/{id}/access_tokens`).
fn installation_token(
    transport: &dyn darkrun_vcs::HttpTransport,
    jwt: &str,
    installation_id: u64,
) -> Result<String, String> {
    let url = format!(
        "{}/app/installations/{}/access_tokens",
        Provider::GitHub.api_base(),
        installation_id,
    );
    let request = app_authorize(HttpRequest::post(url), jwt);
    let response = transport.execute(request).map_err(|e| e.to_string())?;
    if !response.is_success() {
        return Err(format!(
            "minting installation token failed ({})",
            response.status
        ));
    }
    let value: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    value
        .get("token")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| "installation-token response carried no `token`".to_string())
}

/// List an installation's repositories (`GET /installation/repositories`),
/// normalized (runs left empty; the caller fills them per repo). Walks every
/// `?page=N` until a short page ends the list (bounded by [`APP_LIST_MAX_PAGES`])
/// so an installation with more than one page of repos is fully listed.
///
/// The response is an OBJECT `{ total_count, repositories: [...] }` (not a bare
/// array), so both the accumulation and the short-page test read `.repositories`.
fn list_installation_repos(
    transport: &dyn darkrun_vcs::HttpTransport,
    token: &str,
) -> Result<Vec<WorkspaceRepo>, String> {
    let mut repos = Vec::new();
    for page in 1..=APP_LIST_MAX_PAGES {
        let url = format!(
            "{}/installation/repositories?per_page={}&page={}",
            Provider::GitHub.api_base(),
            APP_LIST_PER_PAGE,
            page,
        );
        let request = token_authorize(HttpRequest::get(url), token);
        let response = transport.execute(request).map_err(|e| e.to_string())?;
        if !response.is_success() {
            return Err(format!(
                "listing installation repositories failed ({})",
                response.status
            ));
        }
        let value: serde_json::Value = response.json().map_err(|e| e.to_string())?;
        // Short-page detection reads the RAW `.repositories` length (before
        // normalization, which may drop nameless entries).
        let page_len = value
            .get("repositories")
            .and_then(|r| r.as_array())
            .map(Vec::len)
            .unwrap_or(0);
        repos.extend(parse_installation_repos(&value));
        if page_len < APP_LIST_PER_PAGE as usize {
            break;
        }
        if page == APP_LIST_MAX_PAGES {
            tracing::warn!(
                pages = APP_LIST_MAX_PAGES,
                "installation repository list hit the pagination cap; repos past it are unseen",
            );
        }
    }
    Ok(repos)
}

/// Read a repo's `.darkrun/` runs via the installation token — the same
/// committed-tree discovery `sessions.rs` performs, but installation-scoped.
fn read_runs(
    transport: &dyn darkrun_vcs::HttpTransport,
    token: &str,
    full_name: &str,
) -> Result<Vec<DiscoveredSession>, String> {
    let url = tree_url(Provider::GitHub, full_name);
    let request = token_authorize(HttpRequest::get(url), token);
    let response = transport.execute(request).map_err(|e| e.to_string())?;
    // A repo with no `.darkrun/` (or an empty repo) is not an error — no runs.
    if matches!(response.status, 404 | 409) {
        return Ok(Vec::new());
    }
    if !response.is_success() {
        return Err(format!("reading `.darkrun/` tree failed ({})", response.status));
    }
    let value: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    // GitHub returns the whole recursive tree in one response but caps it
    // (100k entries / 7MB), setting `"truncated": true` when it does. That is NOT
    // page-based, so we cannot walk it; a truncated tree means some `.darkrun/`
    // runs may be unseen. Never let that pass silently — surface it.
    if tree_truncated(&value) {
        tracing::warn!(
            repo = %full_name,
            "GitHub git tree was truncated (>100k entries / 7MB); some `.darkrun/` runs may be missing from discovery",
        );
    }
    Ok(parse_sessions(Provider::GitHub, full_name, &value))
}

/// Read one file's decoded text from a repo via the Contents API
/// (`GET /repos/{full_name}/contents/{path}`), or `None` if it is absent.
fn read_blob(
    transport: &dyn darkrun_vcs::HttpTransport,
    token: &str,
    full_name: &str,
    path: &str,
) -> Result<Option<String>, String> {
    let url = format!(
        "{}/repos/{}/contents/{}",
        Provider::GitHub.api_base(),
        full_name,
        path,
    );
    let request = token_authorize(HttpRequest::get(url), token);
    let response = transport.execute(request).map_err(|e| e.to_string())?;
    if response.status == 404 {
        return Ok(None);
    }
    if !response.is_success() {
        return Err(format!("reading `{path}` failed ({})", response.status));
    }
    let value: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    Ok(decode_contents(&value))
}

/// Decode a GitHub Contents API response's base64 `content` into UTF-8 text.
/// `None` when the payload is not a base64 blob (e.g. a directory listing).
fn decode_contents(value: &serde_json::Value) -> Option<String> {
    let encoding = value.get("encoding").and_then(|e| e.as_str())?;
    if encoding != "base64" {
        return None;
    }
    let content = value.get("content").and_then(|c| c.as_str())?;
    // GitHub wraps the base64 at 60 chars with newlines; strip whitespace first.
    let cleaned: String = content.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64_decode(&cleaned)?;
    String::from_utf8(bytes).ok()
}

/// The `.darkrun/<run_id>/state.json` path.
fn state_path(run_id: &str) -> String {
    format!(".darkrun/{run_id}/state.json")
}

/// The `.darkrun/<run_id>/run.md` path.
fn run_md_path(run_id: &str) -> String {
    format!(".darkrun/{run_id}/run.md")
}

/// Normalize `GET /app/installations` JSON (a top-level array) into
/// [`Installation`]s, skipping malformed entries.
fn parse_installations(value: &serde_json::Value) -> Vec<Installation> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| serde_json::from_value::<Installation>(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a signed-in user may see an installation's repos.
///
/// - A **User** installation is covered when its account IS the caller — matched
///   on the numeric GitHub id (stable), falling back to a case-insensitive login
///   match. An org account never matches a user identity this way.
/// - An **Organization** installation is covered only when the caller is a
///   verified member of that org: their numeric id appears in the org's member
///   list, read via the org installation `token`.
///
/// **Fail safe.** If the org membership check errors (missing `Members: read`,
/// a 403, a timeout, a transport failure, or an unparsable body), the org is
/// treated as *not covered* — this returns `false` and logs a `tracing::warn`.
/// A permission gap can never widen exposure.
fn installation_covers_user(
    transport: &dyn darkrun_vcs::HttpTransport,
    token: &str,
    account: &InstallationAccount,
    identity: &GitHubIdentity,
) -> bool {
    if account.account_type.eq_ignore_ascii_case("organization") {
        return org_membership_confirmed(transport, token, &account.login, identity);
    }
    account_matches_user(account, identity)
}

/// The single installation whose account owns `owner` (login match), if any —
/// used to resolve which installation can read a specific repo. Access is gated
/// separately by [`installation_covers_user`]; this only resolves the owner.
fn installation_for_owner<'a>(
    installations: &'a [Installation],
    owner: &str,
) -> Option<&'a Installation> {
    installations
        .iter()
        .find(|inst| inst.account.login.eq_ignore_ascii_case(owner))
}

/// Whether an installation's *user* account is the signed-in user (id first,
/// then login). Org accounts never match a user identity here.
fn account_matches_user(account: &InstallationAccount, identity: &GitHubIdentity) -> bool {
    if account.account_type.eq_ignore_ascii_case("organization") {
        return false;
    }
    if account.id != 0 && account.id.to_string() == identity.user_id {
        return true;
    }
    match &identity.login {
        Some(login) => !account.login.is_empty() && account.login.eq_ignore_ascii_case(login),
        None => false,
    }
}

/// Confirm the caller is a member of `org` by listing the org's members via the
/// org installation `token` and matching the caller's numeric id.
///
/// Returns `true` only on a *confirmed* match. Any error path — the members call
/// failing (e.g. the App lacks `Members: read`, or a 403/timeout), an unparsable
/// body, or the caller carrying no numeric id — returns `false` and logs a
/// `tracing::warn` naming the org and the reason. This is the fail-safe gate:
/// on doubt, exclude the org.
fn org_membership_confirmed(
    transport: &dyn darkrun_vcs::HttpTransport,
    token: &str,
    org: &str,
    identity: &GitHubIdentity,
) -> bool {
    let Ok(target_id) = identity.user_id.parse::<u64>() else {
        tracing::warn!(
            org = %org,
            reason = "caller has no numeric GitHub id",
            "skipping org: cannot verify membership",
        );
        return false;
    };
    match list_org_member_ids(transport, token, org) {
        Ok(ids) => ids.contains(&target_id),
        Err(reason) => {
            tracing::warn!(
                org = %org,
                %reason,
                "skipping org: membership check failed (App needs `Members: read`?)",
            );
            false
        }
    }
}

/// List an org's member numeric ids via `GET /orgs/{org}/members`, paginating up
/// to [`MEMBERS_MAX_PAGES`] pages. Any non-success status is an error (the caller
/// fails safe on it). If the cap is hit, logs a `tracing::warn` (the remaining
/// members are unseen, so a member past the cap is treated as not-a-member —
/// excluding, never over-including).
fn list_org_member_ids(
    transport: &dyn darkrun_vcs::HttpTransport,
    token: &str,
    org: &str,
) -> Result<std::collections::HashSet<u64>, String> {
    let mut ids = std::collections::HashSet::new();
    for page in 1..=MEMBERS_MAX_PAGES {
        let url = format!(
            "{}/orgs/{}/members?per_page={}&page={}",
            Provider::GitHub.api_base(),
            org,
            MEMBERS_PER_PAGE,
            page,
        );
        let request = token_authorize(HttpRequest::get(url), token);
        let response = transport.execute(request).map_err(|e| e.to_string())?;
        if !response.is_success() {
            return Err(format!("listing org members failed ({})", response.status));
        }
        let value: serde_json::Value = response.json().map_err(|e| e.to_string())?;
        let page_ids = parse_member_ids(&value);
        let page_len = page_ids.len();
        ids.extend(page_ids);
        // A short page (fewer than a full page) is the last page.
        if page_len < MEMBERS_PER_PAGE as usize {
            return Ok(ids);
        }
        if page == MEMBERS_MAX_PAGES {
            tracing::warn!(
                org = %org,
                pages = MEMBERS_MAX_PAGES,
                "org member list hit the pagination cap; members past it are unseen (treated as non-members)",
            );
        }
    }
    Ok(ids)
}

/// Parse the numeric `id` of each member from a `GET /orgs/{org}/members` page
/// (a top-level array of user objects), skipping any entry without a numeric id.
fn parse_member_ids(value: &serde_json::Value) -> Vec<u64> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("id").and_then(|id| id.as_u64()))
                .collect()
        })
        .unwrap_or_default()
}

/// Normalize `GET /installation/repositories` JSON (`{ repositories: [...] }`)
/// into [`WorkspaceRepo`]s (runs empty; filled per repo by the caller).
fn parse_installation_repos(value: &serde_json::Value) -> Vec<WorkspaceRepo> {
    let Some(items) = value.get("repositories").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    items.iter().filter_map(parse_repo).collect()
}

/// Normalize one repository object, or `None` if it lacks a name.
fn parse_repo(item: &serde_json::Value) -> Option<WorkspaceRepo> {
    let str_field = |key: &str| item.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let name = str_field("name")?;
    Some(WorkspaceRepo {
        full_name: str_field("full_name").unwrap_or_else(|| name.clone()),
        url: str_field("html_url").unwrap_or_default(),
        name,
        provider: Provider::GitHub,
        runs: Vec::new(),
    })
}

/// The full committed state of one run, read from its `.darkrun/<id>/` tree.
///
/// Read-only projection of what the git tree reveals — deliberately a lean,
/// dependency-light shape (this crate does not import the engine's domain
/// types), assembled from `state.json` (station/phase position) and `run.md`
/// frontmatter (title / factory / status).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CommittedRun {
    /// The run id — the `.darkrun/<run_id>/` directory name.
    pub run_id: String,
    /// The owner-qualified repository the run lives in.
    pub repo: String,
    /// Resolved display title (falls back to the run id).
    pub title: String,
    /// The factory driving the run, if recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factory: Option<String>,
    /// The station the run currently sits on, if recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_station: Option<String>,
    /// Lifecycle status (display string), if recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Every station the run walks, in `state.json` order, with its status.
    pub stations: Vec<CommittedStation>,
}

/// One station row in a committed run's detail.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CommittedStation {
    /// Station name (e.g. `frame`, `build`).
    pub name: String,
    /// Lifecycle status (display string).
    pub status: String,
    /// The station's current phase within its lifecycle, if recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

/// Assemble a [`CommittedRun`] from the raw `state.json` + `run.md` text (either
/// may be absent). Pure — unit-tested without any network.
fn build_committed_run(
    run_id: &str,
    repo: &str,
    state_json: Option<&str>,
    run_md: Option<&str>,
) -> CommittedRun {
    let state: Option<serde_json::Value> = state_json.and_then(|s| serde_json::from_str(s).ok());
    let front = run_md.map(parse_frontmatter).unwrap_or_default();

    let factory = state
        .as_ref()
        .and_then(|s| s.get("factory"))
        .and_then(|f| f.as_str())
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .or_else(|| front.get("factory").cloned());

    let active_station = state
        .as_ref()
        .and_then(|s| s.get("active_station"))
        .and_then(|a| a.as_str())
        .filter(|a| !a.is_empty())
        .map(str::to_string)
        .or_else(|| front.get("active_station").cloned());

    let status = state
        .as_ref()
        .and_then(|s| s.get("status"))
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| front.get("status").cloned());

    let title = front
        .get("title")
        .cloned()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| run_id.to_string());

    let stations = state
        .as_ref()
        .map(parse_stations)
        .unwrap_or_default();

    CommittedRun {
        run_id: run_id.to_string(),
        repo: repo.to_string(),
        title,
        factory,
        active_station,
        status,
        stations,
    }
}

/// Pull the per-station statuses out of a `state.json` value's `stations` map.
/// Each entry's `status` is read as a display string when present.
fn parse_stations(state: &serde_json::Value) -> Vec<CommittedStation> {
    let Some(map) = state.get("stations").and_then(|s| s.as_object()) else {
        return Vec::new();
    };
    map.iter()
        .map(|(name, station)| {
            let status = station
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("pending")
                .to_string();
            let phase = station
                .get("phase")
                .and_then(|p| p.as_str())
                .filter(|p| !p.is_empty())
                .map(str::to_string);
            CommittedStation {
                name: name.clone(),
                status,
                phase,
            }
        })
        .collect()
}

/// Parse the leading YAML-ish frontmatter block of a `run.md` into a flat
/// key→string map. Deliberately minimal: darkrun frontmatter's top-level
/// scalars (`title`, `factory`, `status`, `active_station`) are simple `key:
/// value` lines, which is all the read-only detail view needs. Nested/list keys
/// are ignored.
fn parse_frontmatter(md: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let trimmed = md.trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        return out;
    };
    // The frontmatter runs up to the next `---` line.
    let Some(end) = rest.find("\n---") else {
        return out;
    };
    let block = &rest[..end];
    for line in block.lines() {
        // Only flat, non-indented `key: value` scalars.
        if line.starts_with(char::is_whitespace) || line.trim().is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'').trim();
        if !key.is_empty() && !value.is_empty() {
            out.insert(key.to_string(), value.to_string());
        }
    }
    out
}

/// Minimal, dependency-free standard-base64 decoder (GitHub Contents uses
/// standard base64 with `+`/`/` and `=` padding). Returns `None` on any invalid
/// character. Kept local so the server needs no base64 crate.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: &[u8] = input.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &b in bytes {
        let v = val(b)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkrun_vcs::{HttpResponse, Method, MockTransport};

    fn identity(login: &str, id: &str) -> GitHubIdentity {
        GitHubIdentity {
            login: Some(login.to_string()),
            user_id: id.to_string(),
        }
    }

    /// Identity carrying only a numeric id (no login) — the real workspace shape,
    /// since the Firebase token yields only the numeric GitHub id.
    fn identity_id_only(id: &str) -> GitHubIdentity {
        GitHubIdentity {
            login: None,
            user_id: id.to_string(),
        }
    }

    fn user_account(login: &str, id: u64) -> InstallationAccount {
        InstallationAccount {
            login: login.to_string(),
            id,
            account_type: "User".to_string(),
        }
    }

    fn org_account(login: &str, id: u64) -> InstallationAccount {
        InstallationAccount {
            login: login.to_string(),
            id,
            account_type: "Organization".to_string(),
        }
    }

    /// The `GET /orgs/{org}/members` URL for a page — must match how
    /// [`list_org_member_ids`] builds it so the mock keys line up.
    fn members_url(org: &str, page: u32) -> String {
        format!(
            "{}/orgs/{}/members?per_page={}&page={}",
            Provider::GitHub.api_base(),
            org,
            MEMBERS_PER_PAGE,
            page,
        )
    }

    /// A members-page body: a top-level array of `{ "id": ... }` user objects.
    fn members_body(ids: &[u64]) -> Vec<u8> {
        let arr: Vec<serde_json::Value> = ids
            .iter()
            .map(|id| serde_json::json!({ "id": id, "login": format!("u{id}") }))
            .collect();
        serde_json::to_vec(&serde_json::Value::Array(arr)).unwrap()
    }

    #[test]
    fn app_jwt_claims_backdate_iat_and_carry_issuer() {
        let claims = app_jwt_claims("12345", 1_000);
        assert_eq!(claims.iss, "12345");
        assert_eq!(claims.iat, 1_000 - APP_JWT_IAT_SKEW_SECS);
        assert_eq!(claims.exp, 1_000 + APP_JWT_TTL_SECS);
        // Never underflow near the epoch.
        assert_eq!(app_jwt_claims("1", 10).iat, 0);
    }

    #[test]
    fn normalize_pem_expands_escaped_newlines_only_when_needed() {
        assert_eq!(normalize_pem("a\\nb"), "a\nb");
        // A real multi-line PEM is untouched.
        assert_eq!(normalize_pem("a\nb"), "a\nb");
        // No escapes → unchanged.
        assert_eq!(normalize_pem("abc"), "abc");
    }

    #[test]
    fn parse_installations_reads_id_and_account() {
        let body = serde_json::json!([
            { "id": 42, "account": { "login": "jwaldrip", "id": 7, "type": "User" } },
            { "id": 43, "account": { "login": "acme", "id": 9, "type": "Organization" } },
            { "garbage": true },
        ]);
        let insts = parse_installations(&body);
        assert_eq!(insts.len(), 2);
        assert_eq!(insts[0].id, 42);
        assert_eq!(insts[0].account.login, "jwaldrip");
        assert_eq!(insts[1].account.account_type, "Organization");
    }

    #[test]
    fn account_matches_user_by_id_then_login_and_never_org() {
        // id match (login case differs — id wins).
        assert!(account_matches_user(
            &user_account("JWaldrip", 7),
            &identity("someone-else", "7"),
        ));
        // Falls back to a case-insensitive login match when the id is absent (0).
        assert!(account_matches_user(
            &user_account("jwaldrip", 0),
            &identity("JWALDRIP", "0"),
        ));
        // An org account is never a user match, even with a matching id.
        assert!(!account_matches_user(
            &org_account("acme", 9),
            &identity("acme", "9"),
        ));
    }

    #[test]
    fn installation_covers_own_user_account_without_any_http() {
        // A User installation that IS the caller is covered by id alone, with no
        // members call — the mock is empty, so any HTTP would error out.
        let mock = MockTransport::new();
        let account = user_account("jwaldrip", 7);
        assert!(installation_covers_user(
            &mock,
            "tok",
            &account,
            &identity_id_only("7"),
        ));
        // A different user is not covered.
        assert!(!installation_covers_user(
            &mock,
            "tok",
            &account,
            &identity_id_only("999"),
        ));
        // No members call was made for a User installation.
        assert!(mock.requests().is_empty());
    }

    #[test]
    fn installation_covers_org_when_user_is_a_member() {
        let mock = MockTransport::new();
        // One short page of members that includes the caller's id (42).
        mock.expect(
            Method::Get,
            members_url("darkrun-ai", 1),
            HttpResponse::new(200, members_body(&[7, 42, 100])),
        );
        assert!(installation_covers_user(
            &mock,
            "org-tok",
            &org_account("darkrun-ai", 555),
            &identity_id_only("42"),
        ));
    }

    #[test]
    fn installation_excludes_org_when_user_is_not_a_member() {
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            members_url("darkrun-ai", 1),
            HttpResponse::new(200, members_body(&[7, 100])),
        );
        assert!(!installation_covers_user(
            &mock,
            "org-tok",
            &org_account("darkrun-ai", 555),
            &identity_id_only("42"),
        ));
    }

    #[test]
    fn installation_excludes_org_when_membership_call_fails() {
        // A 403 (App lacks `Members: read`) must fail safe → excluded.
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            members_url("darkrun-ai", 1),
            HttpResponse::new(403, br#"{"message":"Resource not accessible by integration"}"#.to_vec()),
        );
        assert!(!installation_covers_user(
            &mock,
            "org-tok",
            &org_account("darkrun-ai", 555),
            &identity_id_only("42"),
        ));
    }

    #[test]
    fn installation_excludes_org_when_transport_errors() {
        // No mock response queued → the transport returns an error → fail safe.
        let mock = MockTransport::new();
        assert!(!installation_covers_user(
            &mock,
            "org-tok",
            &org_account("darkrun-ai", 555),
            &identity_id_only("42"),
        ));
    }

    #[test]
    fn org_membership_excluded_when_caller_has_no_numeric_id() {
        // A non-numeric id can never match a numeric member id, and no HTTP is
        // even attempted (the mock is empty).
        let mock = MockTransport::new();
        assert!(!org_membership_confirmed(
            &mock,
            "org-tok",
            "darkrun-ai",
            &identity_id_only("not-a-number"),
        ));
        assert!(mock.requests().is_empty());
    }

    #[test]
    fn list_org_member_ids_paginates_until_a_short_page() {
        let mock = MockTransport::new();
        // A full first page (100 ids) forces a second fetch; the second page is
        // short (2 ids), ending pagination.
        let full: Vec<u64> = (1..=MEMBERS_PER_PAGE as u64).collect();
        mock.expect(
            Method::Get,
            members_url("bigorg", 1),
            HttpResponse::new(200, members_body(&full)),
        );
        mock.expect(
            Method::Get,
            members_url("bigorg", 2),
            HttpResponse::new(200, members_body(&[9_001, 9_002])),
        );
        let ids = list_org_member_ids(&mock, "org-tok", "bigorg").unwrap();
        assert!(ids.contains(&1));
        assert!(ids.contains(&9_002));
        assert_eq!(ids.len(), MEMBERS_PER_PAGE as usize + 2);
        // Exactly two pages were fetched (the short second page stops paging).
        assert_eq!(mock.requests().len(), 2);
    }

    #[test]
    fn list_org_member_ids_surfaces_a_non_success_status() {
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            members_url("acme", 1),
            HttpResponse::new(404, br#"{"message":"Not Found"}"#.to_vec()),
        );
        let err = list_org_member_ids(&mock, "org-tok", "acme").unwrap_err();
        assert!(err.contains("404"), "error should carry the status: {err}");
    }

    #[test]
    fn parse_member_ids_reads_ids_and_skips_idless_entries() {
        let body = serde_json::json!([
            { "id": 7, "login": "a" },
            { "login": "no-id" },
            { "id": 42, "login": "c" },
        ]);
        let ids = parse_member_ids(&body);
        assert_eq!(ids, vec![7, 42]);
        // A non-array (e.g. an error object) yields no ids.
        assert!(parse_member_ids(&serde_json::json!({ "message": "Bad" })).is_empty());
    }

    #[test]
    fn installation_for_owner_matches_login_case_insensitively() {
        let insts = parse_installations(&serde_json::json!([
            { "id": 5, "account": { "login": "jwaldrip", "id": 7, "type": "User" } },
        ]));
        assert_eq!(installation_for_owner(&insts, "JWaldrip").unwrap().id, 5);
        assert!(installation_for_owner(&insts, "nobody").is_none());
    }

    #[test]
    fn parse_installation_repos_normalizes_and_skips_nameless() {
        let body = serde_json::json!({
            "total_count": 2,
            "repositories": [
                { "name": "darkrun", "full_name": "jwaldrip/darkrun", "html_url": "https://github.com/jwaldrip/darkrun" },
                { "full_name": "no/name" },
            ]
        });
        let repos = parse_installation_repos(&body);
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "darkrun");
        assert_eq!(repos[0].full_name, "jwaldrip/darkrun");
        assert_eq!(repos[0].provider, Provider::GitHub);
        assert!(repos[0].runs.is_empty());
    }

    #[test]
    fn parse_installation_repos_tolerates_missing_key() {
        assert!(parse_installation_repos(&serde_json::json!({ "message": "Bad" })).is_empty());
    }

    /// The `GET /installation/repositories` URL for a page — must match how
    /// [`list_installation_repos`] builds it so the mock keys line up.
    fn installation_repos_url(page: u32) -> String {
        format!(
            "{}/installation/repositories?per_page={}&page={}",
            Provider::GitHub.api_base(),
            APP_LIST_PER_PAGE,
            page,
        )
    }

    /// An installation-repositories page body: the `{ total_count, repositories }`
    /// OBJECT shape GitHub returns (NOT a bare array).
    fn installation_repos_body(names: &[&str]) -> Vec<u8> {
        let repos: Vec<serde_json::Value> = names
            .iter()
            .map(|n| {
                serde_json::json!({
                    "name": n,
                    "full_name": format!("acme/{n}"),
                    "html_url": format!("https://github.com/acme/{n}"),
                })
            })
            .collect();
        serde_json::to_vec(&serde_json::json!({
            "total_count": repos.len(),
            "repositories": repos,
        }))
        .unwrap()
    }

    #[test]
    fn list_installation_repos_concatenates_pages_until_a_short_page() {
        let mock = MockTransport::new();
        // A full first page (100 repos) forces page 2; the second page is short
        // (2 repos), ending pagination.
        let full: Vec<String> = (0..APP_LIST_PER_PAGE).map(|i| format!("r-{i}")).collect();
        let full_refs: Vec<&str> = full.iter().map(String::as_str).collect();
        mock.expect(
            Method::Get,
            installation_repos_url(1),
            HttpResponse::new(200, installation_repos_body(&full_refs)),
        );
        mock.expect(
            Method::Get,
            installation_repos_url(2),
            HttpResponse::new(200, installation_repos_body(&["last-a", "last-b"])),
        );
        let repos = list_installation_repos(&mock, "inst-tok").unwrap();
        // Both pages are concatenated.
        assert_eq!(repos.len(), APP_LIST_PER_PAGE as usize + 2);
        assert_eq!(repos[0].name, "r-0");
        assert_eq!(repos.last().unwrap().name, "last-b");
        // Exactly two pages were fetched (the short second page stops paging).
        assert_eq!(mock.requests().len(), 2);
    }

    #[test]
    fn list_installation_repos_single_short_page_makes_one_request() {
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            installation_repos_url(1),
            HttpResponse::new(200, installation_repos_body(&["only-a", "only-b"])),
        );
        let repos = list_installation_repos(&mock, "inst-tok").unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].name, "only-a");
        // A short first page ends pagination immediately — exactly one request.
        assert_eq!(mock.requests().len(), 1);
    }

    /// The `GET /app/installations` URL for a page.
    fn installations_url(page: u32) -> String {
        format!(
            "{}/app/installations?per_page={}&page={}",
            Provider::GitHub.api_base(),
            APP_LIST_PER_PAGE,
            page,
        )
    }

    /// An `/app/installations` page body: a bare array of `count` User
    /// installations with ids counting up from `start_id`.
    fn installations_body(count: usize, start_id: u64) -> Vec<u8> {
        let arr: Vec<serde_json::Value> = (0..count)
            .map(|i| {
                let id = start_id + i as u64;
                serde_json::json!({
                    "id": id,
                    "account": { "login": format!("u{id}"), "id": id, "type": "User" },
                })
            })
            .collect();
        serde_json::to_vec(&serde_json::Value::Array(arr)).unwrap()
    }

    #[test]
    fn list_installations_walks_pages_until_a_short_page() {
        let mock = MockTransport::new();
        // A full first page forces page 2; the short second page ends paging.
        mock.expect(
            Method::Get,
            installations_url(1),
            HttpResponse::new(200, installations_body(APP_LIST_PER_PAGE as usize, 1)),
        );
        mock.expect(
            Method::Get,
            installations_url(2),
            HttpResponse::new(200, installations_body(3, 1_000)),
        );
        let insts = list_installations(&mock, "app-jwt").unwrap();
        assert_eq!(insts.len(), APP_LIST_PER_PAGE as usize + 3);
        assert_eq!(mock.requests().len(), 2);
    }

    #[test]
    fn base64_decode_roundtrips_known_vectors() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode("").unwrap(), b"");
        // Whitespace must be pre-stripped by the caller; a bad char is None.
        assert!(base64_decode("aGVsbG8!").is_none());
    }

    #[test]
    fn decode_contents_reads_base64_blob() {
        let body = serde_json::json!({
            "encoding": "base64",
            // "hello" wrapped like GitHub does (with a newline).
            "content": "aGVs\nbG8=",
        });
        assert_eq!(decode_contents(&body).as_deref(), Some("hello"));
        // A non-base64 payload (directory listing) yields None.
        assert!(decode_contents(&serde_json::json!({ "encoding": "none" })).is_none());
    }

    #[test]
    fn parse_frontmatter_reads_flat_scalars() {
        let md = "---\ntitle: Ship the thing\nfactory: software\nstatus: active\nnested:\n  key: v\n---\n\nBody here.";
        let fm = parse_frontmatter(md);
        assert_eq!(fm.get("title").unwrap(), "Ship the thing");
        assert_eq!(fm.get("factory").unwrap(), "software");
        assert_eq!(fm.get("status").unwrap(), "active");
        // Indented (nested) keys are ignored.
        assert!(!fm.contains_key("key"));
        // No frontmatter → empty.
        assert!(parse_frontmatter("no frontmatter here").is_empty());
    }

    #[test]
    fn build_committed_run_prefers_state_then_frontmatter() {
        let state = r#"{
            "factory": "software",
            "active_station": "build",
            "stations": {
                "frame": { "status": "completed", "phase": "checkpoint" },
                "build": { "status": "active", "phase": "manufacture" }
            }
        }"#;
        let run_md = "---\ntitle: My Run\nstatus: active\n---\nbody";
        let run = build_committed_run("run-abc", "jwaldrip/darkrun", Some(state), Some(run_md));
        assert_eq!(run.run_id, "run-abc");
        assert_eq!(run.repo, "jwaldrip/darkrun");
        assert_eq!(run.title, "My Run");
        assert_eq!(run.factory.as_deref(), Some("software"));
        assert_eq!(run.active_station.as_deref(), Some("build"));
        assert_eq!(run.status.as_deref(), Some("active"));
        assert_eq!(run.stations.len(), 2);
        // Stations come from the JSON object (sorted by key via serde_json map order).
        let build = run.stations.iter().find(|s| s.name == "build").unwrap();
        assert_eq!(build.status, "active");
        assert_eq!(build.phase.as_deref(), Some("manufacture"));
    }

    #[test]
    fn build_committed_run_falls_back_to_run_id_title_when_bare() {
        // No state.json, no run.md → a minimal but valid detail.
        let run = build_committed_run("run-xyz", "a/b", None, None);
        assert_eq!(run.title, "run-xyz");
        assert!(run.factory.is_none());
        assert!(run.active_station.is_none());
        assert!(run.status.is_none());
        assert!(run.stations.is_empty());
    }

    #[test]
    fn paths_are_run_scoped() {
        assert_eq!(state_path("run-1"), ".darkrun/run-1/state.json");
        assert_eq!(run_md_path("run-1"), ".darkrun/run-1/run.md");
    }
}
