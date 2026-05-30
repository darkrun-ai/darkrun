//! REST clients for GitHub Pull Requests and GitLab Merge Requests, plus the
//! unified [`create_change_request`] entry point.
//!
//! Every call goes through the injectable [`HttpTransport`]. The GitHub and
//! GitLab clients build the provider-specific request shape and parse the
//! provider-specific response into a normalized [`ChangeRequest`].

use crate::error::{Result, VcsError};
use crate::oauth::percent_encode;
use crate::provider::{Credential, Provider};
use crate::remote::RepoCoords;
use crate::transport::{HttpRequest, HttpResponse, HttpTransport};

/// A user-agent string. GitHub rejects API requests without one.
const USER_AGENT: &str = "darkrun-vcs";

/// A created change request (a GitHub PR or a GitLab MR), normalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeRequest {
    /// The provider that owns this change request.
    pub provider: Provider,
    /// The web URL where a reviewer lands.
    pub url: String,
    /// The provider-assigned number (PR number / MR iid).
    pub number: u64,
}

/// Minimal repository facts returned by the get-repo / resolve-project calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInfo {
    /// The provider's numeric id for the repo/project.
    pub id: u64,
    /// The default branch (PR/MR base when the caller does not override).
    pub default_branch: String,
    /// The web URL of the repository.
    pub web_url: String,
}

/// Apply the standard auth + accept headers for `provider` to `request`.
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

/// Turn a non-2xx response into a typed [`VcsError::Api`], extracting the
/// provider's error message where possible.
fn api_error(provider: Provider, response: &HttpResponse) -> VcsError {
    let message = response
        .json::<serde_json::Value>()
        .ok()
        .and_then(|v| {
            v.get("message")
                .or_else(|| v.get("error"))
                .or_else(|| v.get("error_description"))
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .or_else(|| response.text().ok())
        .unwrap_or_default();
    VcsError::Api {
        provider: provider.display_name(),
        status: response.status,
        message,
    }
}

/// GitHub: `GET /repos/{owner}/{repo}`.
pub fn github_get_repo(
    transport: &dyn HttpTransport,
    cred: &Credential,
    coords: &RepoCoords,
) -> Result<RepoInfo> {
    let url = format!(
        "{base}/repos/{owner}/{repo}",
        base = Provider::GitHub.api_base(),
        owner = coords.owner,
        repo = coords.repo,
    );
    let request = authorize(HttpRequest::get(url), Provider::GitHub, cred);
    let response = transport.execute(request)?;
    if !response.is_success() {
        return Err(api_error(Provider::GitHub, &response));
    }
    let value: serde_json::Value = response.json()?;
    Ok(RepoInfo {
        id: value.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
        default_branch: value
            .get("default_branch")
            .and_then(|v| v.as_str())
            .unwrap_or("main")
            .to_string(),
        web_url: value
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

/// GitHub: `POST /repos/{owner}/{repo}/pulls`.
pub fn github_create_pull_request(
    transport: &dyn HttpTransport,
    cred: &Credential,
    coords: &RepoCoords,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
) -> Result<ChangeRequest> {
    let url = format!(
        "{base}/repos/{owner}/{repo}/pulls",
        base = Provider::GitHub.api_base(),
        owner = coords.owner,
        repo = coords.repo,
    );
    let payload = serde_json::json!({
        "title": title,
        "head": head,
        "base": base,
        "body": body,
    });
    let request =
        authorize(HttpRequest::post(url), Provider::GitHub, cred).json_body(&payload)?;
    let response = transport.execute(request)?;
    if !response.is_success() {
        return Err(api_error(Provider::GitHub, &response));
    }
    let value: serde_json::Value = response.json()?;
    Ok(ChangeRequest {
        provider: Provider::GitHub,
        number: value
            .get("number")
            .and_then(|v| v.as_u64())
            .ok_or(VcsError::MissingField("number"))?,
        url: value
            .get("html_url")
            .and_then(|v| v.as_str())
            .ok_or(VcsError::MissingField("html_url"))?
            .to_string(),
    })
}

/// GitLab: resolve a project by its URL-encoded path → [`RepoInfo`].
///
/// `GET /projects/{url-encoded path}`.
pub fn gitlab_resolve_project(
    transport: &dyn HttpTransport,
    cred: &Credential,
    coords: &RepoCoords,
) -> Result<RepoInfo> {
    let encoded = percent_encode(&coords.project_path());
    let url = format!(
        "{base}/projects/{encoded}",
        base = Provider::GitLab.api_base(),
    );
    let request = authorize(HttpRequest::get(url), Provider::GitLab, cred);
    let response = transport.execute(request)?;
    if !response.is_success() {
        return Err(api_error(Provider::GitLab, &response));
    }
    let value: serde_json::Value = response.json()?;
    Ok(RepoInfo {
        id: value
            .get("id")
            .and_then(|v| v.as_u64())
            .ok_or(VcsError::MissingField("id"))?,
        default_branch: value
            .get("default_branch")
            .and_then(|v| v.as_str())
            .unwrap_or("main")
            .to_string(),
        web_url: value
            .get("web_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

/// GitLab: `POST /projects/{id}/merge_requests`.
pub fn gitlab_create_merge_request(
    transport: &dyn HttpTransport,
    cred: &Credential,
    project_id: u64,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
) -> Result<ChangeRequest> {
    let url = format!(
        "{base}/projects/{id}/merge_requests",
        base = Provider::GitLab.api_base(),
        id = project_id,
    );
    let payload = serde_json::json!({
        "source_branch": head,
        "target_branch": base,
        "title": title,
        "description": body,
    });
    let request =
        authorize(HttpRequest::post(url), Provider::GitLab, cred).json_body(&payload)?;
    let response = transport.execute(request)?;
    if !response.is_success() {
        return Err(api_error(Provider::GitLab, &response));
    }
    let value: serde_json::Value = response.json()?;
    Ok(ChangeRequest {
        provider: Provider::GitLab,
        number: value
            .get("iid")
            .and_then(|v| v.as_u64())
            .ok_or(VcsError::MissingField("iid"))?,
        url: value
            .get("web_url")
            .and_then(|v| v.as_str())
            .ok_or(VcsError::MissingField("web_url"))?
            .to_string(),
    })
}

/// Create a change request on `provider`, dispatching to the PR or MR client.
///
/// For GitLab this first resolves the project to obtain its numeric id, then
/// opens the merge request against it. For GitHub it posts the pull request
/// directly against `owner/repo`.
#[allow(clippy::too_many_arguments)]
pub fn create_change_request(
    transport: &dyn HttpTransport,
    provider: Provider,
    cred: &Credential,
    coords: &RepoCoords,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
) -> Result<ChangeRequest> {
    match provider {
        Provider::GitHub => {
            github_create_pull_request(transport, cred, coords, head, base, title, body)
        }
        Provider::GitLab => {
            let project = gitlab_resolve_project(transport, cred, coords)?;
            gitlab_create_merge_request(transport, cred, project.id, head, base, title, body)
        }
    }
}
