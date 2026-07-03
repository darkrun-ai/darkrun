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
//! token (login + numeric id, see [`crate::firebase_auth`]), the server selects
//! the installation(s) whose account is that user (or, best-effort, an org the
//! user's installations cover) and returns their repos — each already carrying
//! its `.darkrun/` runs, read the same committed-tree way `sessions.rs` reads
//! them.
//!
//! Everything rides the injectable [`HttpTransport`](darkrun_vcs::HttpTransport)
//! seam, so the pure parts — JWT claims, installation selection, repo/run
//! normalization — are unit-tested fully offline; only the signing + network
//! calls need real credentials.
//!
//! Absent config (`GITHUB_APP_ID` / `GITHUB_APP_PRIVATE_KEY` unset) is a
//! *disabled feature*, not a crash: [`GitHubApp::from_env`] returns `None` and
//! the workspace endpoints answer with a clear "not configured" error.

use darkrun_vcs::{HttpRequest, Provider};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::sessions::{parse_sessions, tree_url, DiscoveredSession};

/// A user-agent string. GitHub rejects API requests without one.
const USER_AGENT: &str = "darkrun-web";

/// The App JWT's lifetime, in seconds. GitHub caps an App JWT at 10 minutes; we
/// use 9 to leave headroom for clock skew on GitHub's side.
const APP_JWT_TTL_SECS: u64 = 540;

/// Backdate the App JWT's `iat` by this many seconds to tolerate our clock
/// running slightly ahead of GitHub's (GitHub rejects a future `iat`).
const APP_JWT_IAT_SKEW_SECS: u64 = 60;

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
    /// runs), assembled from every installation whose account matches the user.
    ///
    /// Pure orchestration over the synchronous
    /// [`HttpTransport`](darkrun_vcs::HttpTransport) seam, so a caller runs it on
    /// the blocking pool exactly like `repos.rs`/`sessions.rs`.
    ///
    /// Steps: sign an App JWT → list installations → keep the installation(s)
    /// this user owns (login/id match) → for each, mint an installation token,
    /// list its repositories, and read each repo's `.darkrun/` runs.
    #[cfg(not(tarpaulin_include))] // network orchestration (parts below are tested)
    pub fn workspace(
        &self,
        transport: &dyn darkrun_vcs::HttpTransport,
        identity: &GitHubIdentity,
        now: u64,
    ) -> Result<Vec<WorkspaceRepo>, String> {
        let jwt = self.app_jwt(now)?;
        let installations = list_installations(transport, &jwt)?;
        let mine = installations_for(&installations, identity);

        let mut repos = Vec::new();
        for inst in mine {
            let token = installation_token(transport, &jwt, inst.id)?;
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
    #[cfg(not(tarpaulin_include))] // network orchestration (parts below are tested)
    pub fn run_detail(
        &self,
        transport: &dyn darkrun_vcs::HttpTransport,
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

/// List every installation of the App (`GET /app/installations`).
fn list_installations(
    transport: &dyn darkrun_vcs::HttpTransport,
    jwt: &str,
) -> Result<Vec<Installation>, String> {
    let url = format!("{}/app/installations?per_page=100", Provider::GitHub.api_base());
    let request = app_authorize(HttpRequest::get(url), jwt);
    let response = transport.execute(request).map_err(|e| e.to_string())?;
    if !response.is_success() {
        return Err(format!(
            "listing App installations failed ({})",
            response.status
        ));
    }
    let value: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    Ok(parse_installations(&value))
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
/// normalized (runs left empty; the caller fills them per repo).
fn list_installation_repos(
    transport: &dyn darkrun_vcs::HttpTransport,
    token: &str,
) -> Result<Vec<WorkspaceRepo>, String> {
    let url = format!(
        "{}/installation/repositories?per_page=100",
        Provider::GitHub.api_base(),
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
    Ok(parse_installation_repos(&value))
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

/// Select the installations whose account IS the signed-in user — matched on the
/// numeric GitHub id (stable), falling back to a case-insensitive login match.
///
/// A user-account installation is the user's own repos. Org installations the
/// user's *own* installation doesn't cover are NOT included here: the App can't,
/// from installations alone, prove the user is a member of that org's account,
/// so returning them would leak repos. (Selecting org installations the user
/// belongs to needs a membership check — a documented limitation.)
fn installations_for<'a>(
    installations: &'a [Installation],
    identity: &GitHubIdentity,
) -> Vec<&'a Installation> {
    installations
        .iter()
        .filter(|inst| account_matches_user(&inst.account, identity))
        .collect()
}

/// The single installation whose account owns `owner` (login match), if any —
/// used to resolve which installation can read a specific repo.
fn installation_for_owner<'a>(
    installations: &'a [Installation],
    owner: &str,
) -> Option<&'a Installation> {
    installations
        .iter()
        .find(|inst| inst.account.login.eq_ignore_ascii_case(owner))
}

/// Whether an installation's account is the signed-in user (id first, then
/// login). Org accounts never match a user identity here.
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
            CommittedStation {
                name: name.clone(),
                status,
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

    fn identity(login: &str, id: &str) -> GitHubIdentity {
        GitHubIdentity {
            login: Some(login.to_string()),
            user_id: id.to_string(),
        }
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
    fn installations_for_matches_user_by_id_then_login() {
        let insts = parse_installations(&serde_json::json!([
            { "id": 1, "account": { "login": "JWaldrip", "id": 7, "type": "User" } },
            { "id": 2, "account": { "login": "acme", "id": 9, "type": "Organization" } },
        ]));
        // id match (login case differs — id wins).
        let mine = installations_for(&insts, &identity("someone-else", "7"));
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].id, 1);
        // Org installations are never a user match.
        let mine = installations_for(&insts, &identity("acme", "9"));
        assert!(mine.is_empty());
    }

    #[test]
    fn installations_for_falls_back_to_login_when_id_absent() {
        let insts = parse_installations(&serde_json::json!([
            { "id": 1, "account": { "login": "jwaldrip", "type": "User" } },
        ]));
        let mine = installations_for(&insts, &identity("JWALDRIP", "0"));
        assert_eq!(mine.len(), 1);
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
                "frame": { "status": "completed" },
                "build": { "status": "active" }
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
        assert!(run.stations.iter().any(|s| s.name == "build" && s.status == "active"));
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
