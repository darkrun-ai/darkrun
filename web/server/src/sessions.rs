//! `GET /api/repos/sessions` — the read-side session discovery for a repo.
//!
//! The access model says: NO state sync. To surface a repo's darkrun runs
//! without a live engine, read the repo's **committed `.darkrun/` tree** from
//! the provider, given the caller's repo access. This endpoint does exactly
//! that: with the caller's provider OAuth access token (the same bearer
//! credential `GET /api/repos` uses), it lists the repo's git tree, filters to
//! `.darkrun/<run>/`, and returns a normalized list of runs.
//!
//! It is strictly **read-only** — it never writes to the repo or to the engine.
//! Reaching the *live* engine (the relay attach / write path) is a separate,
//! later concern; this only answers "what runs are committed here?".
//!
//! - GitHub: `GET /repos/{owner}/{repo}/git/trees/HEAD?recursive=1` — one call
//!   returns the whole tree; we filter the paths under `.darkrun/`.
//! - GitLab: `GET /projects/{id}/repository/tree?path=.darkrun&recursive=true`
//!   — the subtree rooted at `.darkrun`.
//!
//! Resilience: a repo with no `.darkrun/` (a `404` from the tree call, or an
//! empty/filtered tree) is **not an error** — it returns an empty list. The
//! token is used for exactly one upstream call and dropped; the upstream call
//! rides the same injectable [`HttpTransport`](darkrun_vcs::HttpTransport) seam
//! `repos.rs` uses, so this is fully offline-testable.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use darkrun_vcs::{Credential, HttpRequest, Provider};
use serde::{Deserialize, Serialize};

use crate::state::WebState;

/// A user-agent string. GitHub rejects API requests without one.
const USER_AGENT: &str = "darkrun-web";

/// The committed-state directory every darkrun run lives under.
const STATE_DIR: &str = ".darkrun";

/// Entries per page when walking GitLab's `.darkrun` tree
/// (`GET /projects/{}/repository/tree`). Mirrors the `per_page=100` baked into
/// the GitLab branch of [`tree_url`]; a page shorter than this is the last one.
const TREE_PER_PAGE: usize = 100;

/// The most `.darkrun` tree pages we walk for one GitLab repo before stopping.
/// At [`TREE_PER_PAGE`] entries per page this covers 20,000 tree entries (a repo
/// with many runs, each with many artifact blobs); the cap is a safety bound so
/// a misbehaving remote can never spin us forever. If hit, it is logged; entries
/// past it are unseen. GitHub is exempt — it returns the whole tree in one call.
const TREE_MAX_PAGES: u32 = 200;

/// Query for `GET /api/repos/sessions` — which provider the bearer token
/// belongs to and which repository to read the `.darkrun/` tree from.
#[derive(Debug, Deserialize)]
pub struct SessionsQuery {
    /// The provider the access token authenticates against (`github` | `gitlab`).
    pub provider: String,
    /// The owner-qualified repository path (e.g. `jwaldrip/darkrun`).
    pub full_name: String,
}

/// One darkrun run discovered in a repo's committed `.darkrun/` tree.
///
/// Read-only: this is what the git tree reveals, not the live engine's view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredSession {
    /// The run identifier — the `.darkrun/<run_id>/` directory name.
    pub run_id: String,
    /// The owner-qualified repository the run was discovered in.
    pub repo: String,
    /// The provider the repository is hosted on.
    pub provider: Provider,
}

/// `GET /api/repos/sessions?provider=…&full_name=…`
///
/// Reads the provider OAuth access token from the `Authorization: Bearer …`
/// header, reads the repo's committed `.darkrun/` tree from that provider, and
/// returns the normalized list of runs. A repo without `.darkrun/` yields an
/// empty list (not an error). The provider call runs on the blocking pool (the
/// transport seam is synchronous), mirroring `list_repos`.
pub async fn list_sessions(
    State(state): State<WebState>,
    Query(query): Query<SessionsQuery>,
    headers: axum::http::HeaderMap,
) -> Response {
    let Some(provider) = Provider::from_key(&query.provider) else {
        return json_error(
            StatusCode::BAD_REQUEST,
            &format!("`{}` is not a supported provider.", query.provider),
        );
    };

    let full_name = query.full_name.trim().to_string();
    if full_name.is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "A repository (`full_name`) is required.",
        );
    }

    let Some(token) = bearer_token(&headers) else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "A provider access token is required (Authorization: Bearer …).",
        );
    };

    let transport = state.transport.clone();
    let cred = Credential::new(provider, token);

    // The transport seam is synchronous and may block on network I/O; run it off
    // the async reactor, exactly like the repo listing.
    let listed = tokio::task::spawn_blocking(move || {
        fetch_sessions(transport.as_ref(), provider, &cred, &full_name)
    })
    .await;

    match listed {
        Ok(Ok(sessions)) => Json(sessions).into_response(),
        Ok(Err(e)) => {
            tracing::warn!(provider = provider.key(), error = %e, "session discovery failed");
            json_error(
                StatusCode::BAD_GATEWAY,
                "darkrun could not read this repository's runs from the provider.",
            )
        }
        Err(e) => {
            tracing::error!(error = %e, "session discovery task panicked");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Something went wrong reading this repository's runs.",
            )
        }
    }
}

/// Extract the bearer token from an `Authorization` header, if present.
fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// Read `repo`'s committed `.darkrun/` tree from `provider` using `cred`,
/// normalized into runs. A missing `.darkrun/` (a `404`) is not an error.
///
/// The two providers have different tree pagination models, so each has its own
/// fetch: GitHub returns the whole recursive tree in one call (and flags
/// truncation), while GitLab's subtree endpoint is page-based and is walked to
/// the end.
fn fetch_sessions(
    transport: &dyn darkrun_vcs::HttpTransport,
    provider: Provider,
    cred: &Credential,
    repo: &str,
) -> darkrun_vcs::Result<Vec<DiscoveredSession>> {
    match provider {
        Provider::GitHub => fetch_sessions_github(transport, cred, repo),
        Provider::GitLab => fetch_sessions_gitlab(transport, cred, repo),
    }
}

/// GitHub tree discovery: one recursive call returns the whole tree. GitHub caps
/// it (100k entries / 7MB) and sets `"truncated": true` when it does — that is
/// NOT page-based, so we can't walk it, but a silent truncation would drop runs,
/// so we surface it via a `tracing::warn` before normalizing whatever we got.
fn fetch_sessions_github(
    transport: &dyn darkrun_vcs::HttpTransport,
    cred: &Credential,
    repo: &str,
) -> darkrun_vcs::Result<Vec<DiscoveredSession>> {
    let url = tree_url(Provider::GitHub, repo);
    let request = authorize(HttpRequest::get(url), Provider::GitHub, cred);
    let response = transport.execute(request)?;

    // No committed tree at all (`404`/`409` on an empty repo) is a legitimate
    // empty result, not a failure.
    if matches!(response.status, 404 | 409) {
        return Ok(Vec::new());
    }
    if !response.is_success() {
        return Err(darkrun_vcs::VcsError::Api {
            provider: Provider::GitHub.display_name(),
            status: response.status,
            message: response.text().unwrap_or_default(),
        });
    }

    let value: serde_json::Value = response.json()?;
    if tree_truncated(&value) {
        tracing::warn!(
            repo = %repo,
            "GitHub git tree was truncated (>100k entries / 7MB); some `.darkrun/` runs may be missing from discovery",
        );
    }
    Ok(parse_sessions(Provider::GitHub, repo, &value))
}

/// GitLab tree discovery: the `.darkrun` subtree endpoint is page-based
/// (`?page=N` + `per_page`), so walk every page until a short page ends the list
/// (bounded by [`TREE_MAX_PAGES`]). Every page's raw entries are accumulated and
/// handed to [`parse_sessions`] once, so cross-page run-id de-duplication is
/// preserved exactly as the single-page path did it.
fn fetch_sessions_gitlab(
    transport: &dyn darkrun_vcs::HttpTransport,
    cred: &Credential,
    repo: &str,
) -> darkrun_vcs::Result<Vec<DiscoveredSession>> {
    let mut entries: Vec<serde_json::Value> = Vec::new();
    for page in 1..=TREE_MAX_PAGES {
        let url = format!("{}&page={}", tree_url(Provider::GitLab, repo), page);
        let request = authorize(HttpRequest::get(url), Provider::GitLab, cred);
        let response = transport.execute(request)?;

        // A repo with no `.darkrun/` yields a `404` — a clean empty result.
        if matches!(response.status, 404 | 409) {
            break;
        }
        if !response.is_success() {
            return Err(darkrun_vcs::VcsError::Api {
                provider: Provider::GitLab.display_name(),
                status: response.status,
                message: response.text().unwrap_or_default(),
            });
        }

        let value: serde_json::Value = response.json()?;
        let Some(page_entries) = value.as_array() else {
            break;
        };
        let page_len = page_entries.len();
        entries.extend(page_entries.iter().cloned());
        // A short page (fewer than a full page) is the last page.
        if page_len < TREE_PER_PAGE {
            break;
        }
        if page == TREE_MAX_PAGES {
            tracing::warn!(
                repo = %repo,
                pages = TREE_MAX_PAGES,
                "GitLab `.darkrun` tree hit the pagination cap; entries past it are unseen",
            );
        }
    }
    // Normalize + de-duplicate across all pages in one pass over the combined
    // array (the GitLab shape [`parse_sessions`] expects is a bare array).
    Ok(parse_sessions(
        Provider::GitLab,
        repo,
        &serde_json::Value::Array(entries),
    ))
}

/// The git-tree endpoint URL for `repo` on `provider`. Shared with the
/// GitHub-App workspace path (`github_app.rs`), which reads the same
/// `.darkrun/` tree through an installation token.
pub(crate) fn tree_url(provider: Provider, repo: &str) -> String {
    match provider {
        // The whole tree at the default branch (`HEAD`), recursively, in one
        // call; we filter the entries down to `.darkrun/` ourselves.
        Provider::GitHub => format!(
            "{}/repos/{}/git/trees/HEAD?recursive=1",
            provider.api_base(),
            repo,
        ),
        // GitLab takes the project as a URL-encoded path and a subtree root.
        Provider::GitLab => format!(
            "{}/projects/{}/repository/tree?path={}&recursive=true&per_page=100",
            provider.api_base(),
            urlencode(repo),
            STATE_DIR,
        ),
    }
}

/// Whether a GitHub git-tree response was truncated. GitHub sets
/// `"truncated": true` when the recursive tree exceeds its cap (100k entries /
/// 7MB); a truncated tree may be missing `.darkrun/` entries. Shared with the
/// GitHub-App workspace path (`github_app.rs`), which reads the same tree.
pub(crate) fn tree_truncated(value: &serde_json::Value) -> bool {
    value
        .get("truncated")
        .and_then(|t| t.as_bool())
        .unwrap_or(false)
}

/// Percent-encode a repository path for use in a URL path segment (GitLab wants
/// `owner/repo` as a single encoded id).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Apply the standard auth + accept headers for `provider`. Mirrors `repos.rs`
/// so both endpoints speak to the providers identically.
fn authorize(request: HttpRequest, provider: Provider, cred: &Credential) -> HttpRequest {
    let request = request
        .header("Authorization", cred.authorization_header())
        .header("User-Agent", USER_AGENT);
    match provider {
        Provider::GitHub => request
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28"),
        Provider::GitLab => request.header("Accept", "application/json"),
    }
}

/// Normalize a provider's tree JSON into the runs under `.darkrun/`. Shared with
/// the GitHub-App workspace path (`github_app.rs`).
pub(crate) fn parse_sessions(
    provider: Provider,
    repo: &str,
    value: &serde_json::Value,
) -> Vec<DiscoveredSession> {
    // Both providers return tree entries with a `path` (GitHub nests them under
    // `tree`; GitLab returns a top-level array). Collect those paths, then pull
    // the distinct `.darkrun/<run_id>` directory names out of them.
    let entries = match provider {
        Provider::GitHub => value.get("tree").and_then(|t| t.as_array()),
        Provider::GitLab => value.as_array(),
    };
    let Some(entries) = entries else {
        return Vec::new();
    };

    let mut run_ids: Vec<String> = Vec::new();
    for entry in entries {
        let Some(path) = entry.get("path").and_then(|p| p.as_str()) else {
            continue;
        };
        if let Some(run_id) = run_id_from_path(path) {
            if !run_ids.iter().any(|r| r == run_id) {
                run_ids.push(run_id.to_string());
            }
        }
    }

    run_ids
        .into_iter()
        .map(|run_id| DiscoveredSession {
            run_id,
            repo: repo.to_string(),
            provider,
        })
        .collect()
}

/// Pull the `<run_id>` out of a `.darkrun/<run_id>[/…]` tree path, if the path
/// is under the state dir and names a run (not the state dir itself, not a
/// top-level file like `.darkrun/settings.yml`).
fn run_id_from_path(path: &str) -> Option<&str> {
    let rest = path.strip_prefix(STATE_DIR)?.strip_prefix('/')?;
    if rest.is_empty() {
        return None;
    }
    let run_id = rest.split('/').next()?;
    // A bare top-level file under `.darkrun/` (no further segments) is config,
    // not a run — only treat a segment as a run when it's a directory, i.e. the
    // path has something nested beneath it.
    if rest.len() == run_id.len() {
        return None;
    }
    (!run_id.is_empty()).then_some(run_id)
}

/// A JSON error body with `status`.
fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkrun_vcs::{HttpResponse, Method, MockTransport};

    #[test]
    fn github_tree_yields_distinct_runs() {
        let body = serde_json::json!({
            "tree": [
                { "path": ".darkrun", "type": "tree" },
                { "path": ".darkrun/settings.yml", "type": "blob" },
                { "path": ".darkrun/run-abc", "type": "tree" },
                { "path": ".darkrun/run-abc/run.md", "type": "blob" },
                { "path": ".darkrun/run-abc/state.json", "type": "blob" },
                { "path": ".darkrun/run-xyz/run.md", "type": "blob" },
                { "path": "src/main.rs", "type": "blob" },
            ]
        });
        let runs = parse_sessions(Provider::GitHub, "jwaldrip/darkrun", &body);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].run_id, "run-abc");
        assert_eq!(runs[0].repo, "jwaldrip/darkrun");
        assert_eq!(runs[0].provider, Provider::GitHub);
        assert_eq!(runs[1].run_id, "run-xyz");
    }

    #[test]
    fn gitlab_subtree_yields_runs() {
        let body = serde_json::json!([
            { "path": ".darkrun/settings.yml", "type": "blob" },
            { "path": ".darkrun/run-1", "type": "tree" },
            { "path": ".darkrun/run-1/run.md", "type": "blob" },
            { "path": ".darkrun/run-2/state.json", "type": "blob" },
        ]);
        let runs = parse_sessions(Provider::GitLab, "jwaldrip/darkrun", &body);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].run_id, "run-1");
        assert_eq!(runs[1].run_id, "run-2");
        assert_eq!(runs[0].provider, Provider::GitLab);
    }

    #[test]
    fn a_repo_without_darkrun_yields_no_sessions() {
        let body = serde_json::json!({ "tree": [ { "path": "src/main.rs", "type": "blob" } ] });
        assert!(parse_sessions(Provider::GitHub, "a/b", &body).is_empty());
    }

    #[test]
    fn top_level_state_files_are_not_runs() {
        // `.darkrun/settings.yml` is config, not a run; `.darkrun` itself is the
        // dir. Neither should be reported as a run.
        assert_eq!(run_id_from_path(".darkrun"), None);
        assert_eq!(run_id_from_path(".darkrun/settings.yml"), None);
        assert_eq!(run_id_from_path(".darkrun/run-abc"), None);
        assert_eq!(run_id_from_path(".darkrun/run-abc/run.md"), Some("run-abc"));
        assert_eq!(run_id_from_path("src/main.rs"), None);
    }

    #[test]
    fn non_tree_payload_yields_no_sessions() {
        let body = serde_json::json!({ "message": "Not Found" });
        assert!(parse_sessions(Provider::GitHub, "a/b", &body).is_empty());
        let body = serde_json::json!({ "message": "Not Found" });
        assert!(parse_sessions(Provider::GitLab, "a/b", &body).is_empty());
    }

    #[test]
    fn tree_url_is_provider_specific() {
        assert_eq!(
            tree_url(Provider::GitHub, "jwaldrip/darkrun"),
            "https://api.github.com/repos/jwaldrip/darkrun/git/trees/HEAD?recursive=1"
        );
        assert_eq!(
            tree_url(Provider::GitLab, "jwaldrip/darkrun"),
            "https://gitlab.com/api/v4/projects/jwaldrip%2Fdarkrun/repository/tree?path=.darkrun&recursive=true&per_page=100"
        );
    }

    // ---- multi-page fetch (offline via the mock transport) ------------------

    /// The GitLab `.darkrun` tree URL for a page — `tree_url` plus `&page=N`,
    /// exactly as [`fetch_sessions_gitlab`] builds it, so the mock keys line up.
    fn gitlab_tree_page_url(repo: &str, page: u32) -> String {
        format!("{}&page={}", tree_url(Provider::GitLab, repo), page)
    }

    /// A GitLab tree page body: a bare array of `{ path, type: "blob" }` entries.
    fn gitlab_tree_body(paths: &[String]) -> Vec<u8> {
        let arr: Vec<serde_json::Value> = paths
            .iter()
            .map(|p| serde_json::json!({ "path": p, "type": "blob" }))
            .collect();
        serde_json::to_vec(&serde_json::Value::Array(arr)).unwrap()
    }

    #[test]
    fn gitlab_tree_is_walked_across_pages_and_deduped() {
        let repo = "jwaldrip/darkrun";
        let cred = Credential::new(Provider::GitLab, "glpat");
        let mock = MockTransport::new();

        // A full first page: TREE_PER_PAGE blobs, one per run run-000..run-099.
        let page1: Vec<String> = (0..TREE_PER_PAGE)
            .map(|i| format!(".darkrun/run-{i:03}/state.json"))
            .collect();
        // A short second page: a dup of run-099 (cross-page de-dup) + two NEW runs.
        let page2 = vec![
            ".darkrun/run-099/run.md".to_string(),
            ".darkrun/run-100/state.json".to_string(),
            ".darkrun/run-101/run.md".to_string(),
        ];
        mock.expect(
            Method::Get,
            gitlab_tree_page_url(repo, 1),
            HttpResponse::new(200, gitlab_tree_body(&page1)),
        );
        mock.expect(
            Method::Get,
            gitlab_tree_page_url(repo, 2),
            HttpResponse::new(200, gitlab_tree_body(&page2)),
        );

        let runs = fetch_sessions(&mock, Provider::GitLab, &cred, repo).unwrap();
        // 100 from page 1 + 2 NEW from page 2 (run-099 de-duplicated) = 102.
        assert_eq!(runs.len(), TREE_PER_PAGE + 2);
        assert!(runs.iter().any(|r| r.run_id == "run-100"), "page-2 run seen");
        assert!(runs.iter().any(|r| r.run_id == "run-101"), "page-2 run seen");
        // run-099 appears exactly once despite being on both pages.
        assert_eq!(runs.iter().filter(|r| r.run_id == "run-099").count(), 1);
        // Exactly two pages fetched (the short second page stops the walk).
        assert_eq!(mock.requests().len(), 2);
    }

    #[test]
    fn gitlab_tree_single_short_page_makes_one_request() {
        let repo = "jwaldrip/darkrun";
        let cred = Credential::new(Provider::GitLab, "glpat");
        let mock = MockTransport::new();
        let page1 = vec![
            ".darkrun/run-1/run.md".to_string(),
            ".darkrun/run-2/state.json".to_string(),
        ];
        mock.expect(
            Method::Get,
            gitlab_tree_page_url(repo, 1),
            HttpResponse::new(200, gitlab_tree_body(&page1)),
        );
        let runs = fetch_sessions(&mock, Provider::GitLab, &cred, repo).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].run_id, "run-1");
        // A short first page ends the walk immediately — exactly one request.
        assert_eq!(mock.requests().len(), 1);
    }

    #[test]
    fn gitlab_tree_missing_darkrun_is_empty_not_an_error() {
        let repo = "acme/empty";
        let cred = Credential::new(Provider::GitLab, "glpat");
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            gitlab_tree_page_url(repo, 1),
            HttpResponse::new(404, br#"{"message":"404 Tree Not Found"}"#.to_vec()),
        );
        let runs = fetch_sessions(&mock, Provider::GitLab, &cred, repo).unwrap();
        assert!(runs.is_empty());
        assert_eq!(mock.requests().len(), 1);
    }

    #[test]
    fn github_tree_truncation_is_surfaced_and_still_parses() {
        // A truncated tree must not silently drop runs: the entries present are
        // still parsed, and `tree_truncated` flags the cap (the log warns).
        let truncated = serde_json::json!({
            "truncated": true,
            "tree": [ { "path": ".darkrun/run-abc/run.md", "type": "blob" } ]
        });
        assert!(tree_truncated(&truncated));
        let repo = "jwaldrip/darkrun";
        let cred = Credential::new(Provider::GitHub, "gho");
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            tree_url(Provider::GitHub, repo),
            HttpResponse::new(200, serde_json::to_vec(&truncated).unwrap()),
        );
        let runs = fetch_sessions(&mock, Provider::GitHub, &cred, repo).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_id, "run-abc");
        // GitHub is a single call — no pagination.
        assert_eq!(mock.requests().len(), 1);
        // A tree with no `truncated` key reads false.
        assert!(!tree_truncated(&serde_json::json!({ "tree": [] })));
    }
}
