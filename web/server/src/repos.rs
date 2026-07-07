//! `GET /api/repos` — the standalone web app's repository portfolio.
//!
//! The dashboard at app.darkrun.ai asks "what repos can my linked accounts
//! reach?" This endpoint answers it: given the caller's **provider OAuth access
//! token** (the same token kind the OAuth broker hands the CLI — minted by the
//! Firebase sign-in's provider credential, see `web/app/js/firebase-login.js`),
//! it calls the provider's list-repos API and returns a normalized list.
//!
//! The token arrives as a bearer credential the server never persists; it is
//! used for exactly one upstream call and dropped. The upstream call rides the
//! same injectable [`HttpTransport`](darkrun_vcs::HttpTransport) seam the OAuth
//! token exchange uses, so this is fully offline-testable.
//!
//! - GitHub: `GET /user/repos?per_page=100&sort=updated`
//! - GitLab: `GET /api/v4/projects?membership=true&per_page=100`
//!
//! This endpoint is only the repo list; a repo's committed `.darkrun/` runs are
//! discovered separately by `GET /api/repos/sessions` (see `sessions.rs`).

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

/// Repositories per page. Both providers cap the list endpoints at 100 and
/// paginate with `?page=N` over a bare array, so the same page-walk covers both;
/// this mirrors the `per_page=100` baked into [`repos_url`]. A page shorter than
/// this is the last one.
const REPOS_PER_PAGE: usize = 100;

/// The most pages of repositories we walk before stopping. At [`REPOS_PER_PAGE`]
/// per page this covers 5,000 repos; the cap is a safety bound so a misbehaving
/// remote can never spin us forever. If hit, it is logged; repos past it are
/// unseen.
const REPOS_MAX_PAGES: u32 = 50;

/// Query for `GET /api/repos` — which provider the bearer token belongs to.
#[derive(Debug, Deserialize)]
pub struct ReposQuery {
    /// The provider the access token authenticates against (`github` | `gitlab`).
    pub provider: String,
}

/// One repository the caller's account can reach, normalized across providers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Repo {
    /// The short repository name (e.g. `darkrun`).
    pub name: String,
    /// The owner-qualified path (e.g. `jwaldrip/darkrun`).
    pub full_name: String,
    /// The web URL where the repository lives.
    pub url: String,
    /// The provider the repository is hosted on.
    pub provider: Provider,
}

/// `GET /api/repos?provider=…`
///
/// Reads the provider OAuth access token from the `Authorization: Bearer …`
/// header, lists the caller's repositories from that provider, and returns the
/// normalized list. The provider call runs on the blocking pool (the transport
/// seam is synchronous), mirroring the OAuth `callback` handler.
pub async fn list_repos(
    State(state): State<WebState>,
    Query(query): Query<ReposQuery>,
    headers: axum::http::HeaderMap,
) -> Response {
    let Some(provider) = Provider::from_key(&query.provider) else {
        return json_error(
            StatusCode::BAD_REQUEST,
            &format!("`{}` is not a supported provider.", query.provider),
        );
    };

    let Some(token) = bearer_token(&headers) else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "A provider access token is required (Authorization: Bearer …).",
        );
    };

    let transport = state.transport.clone();
    let cred = Credential::new(provider, token);

    // The transport seam is synchronous and may block on network I/O; run it off
    // the async reactor, exactly like the OAuth token exchange.
    let listed =
        tokio::task::spawn_blocking(move || fetch_repos(transport.as_ref(), provider, &cred)).await;

    match listed {
        Ok(Ok(repos)) => Json(repos).into_response(),
        Ok(Err(e)) => {
            tracing::warn!(provider = provider.key(), error = %e, "repo listing failed");
            json_error(
                StatusCode::BAD_GATEWAY,
                "darkrun could not list your repositories from the provider.",
            )
        }
        Err(e) => {
            tracing::error!(error = %e, "repo listing task panicked");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Something went wrong listing your repositories.",
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
    let token = raw.strip_prefix("Bearer ").or_else(|| raw.strip_prefix("bearer "))?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// List the caller's repositories from `provider` using `cred`, normalized.
///
/// Both GitHub (`/user/repos`) and GitLab (`/projects`) return a bare array and
/// paginate with `?page=N`, so this walks every page until a short page ends the
/// list (bounded by [`REPOS_MAX_PAGES`]) — a portfolio of more than one page of
/// repos is fully returned rather than truncated at the first 100.
fn fetch_repos(
    transport: &dyn darkrun_vcs::HttpTransport,
    provider: Provider,
    cred: &Credential,
) -> darkrun_vcs::Result<Vec<Repo>> {
    let mut repos = Vec::new();
    for page in 1..=REPOS_MAX_PAGES {
        let url = format!("{}&page={}", repos_url(provider), page);
        let request = authorize(HttpRequest::get(url), provider, cred);
        let response = transport.execute(request)?;
        if !response.is_success() {
            return Err(darkrun_vcs::VcsError::Api {
                provider: provider.display_name(),
                status: response.status,
                message: response.text().unwrap_or_default(),
            });
        }
        let value: serde_json::Value = response.json()?;
        // Short-page detection reads the RAW array length (before normalization,
        // which may drop nameless entries) so a full page is never mistaken for
        // the last one.
        let page_len = value.as_array().map(Vec::len).unwrap_or(0);
        repos.extend(parse_repos(provider, &value));
        if page_len < REPOS_PER_PAGE {
            break;
        }
        if page == REPOS_MAX_PAGES {
            tracing::warn!(
                provider = provider.key(),
                pages = REPOS_MAX_PAGES,
                "repo portfolio hit the pagination cap; repos past it are unseen",
            );
        }
    }
    Ok(repos)
}

/// The list-repos endpoint URL for `provider`.
fn repos_url(provider: Provider) -> String {
    match provider {
        // The authenticated user's own + collaborator repos, newest first.
        Provider::GitHub => format!(
            "{}/user/repos?per_page=100&sort=updated",
            provider.api_base()
        ),
        // Projects the user is a member of (owned + shared).
        Provider::GitLab => format!(
            "{}/projects?membership=true&per_page=100&order_by=last_activity_at",
            provider.api_base()
        ),
    }
}

/// Apply the standard auth + accept headers for `provider`. Mirrors the
/// `darkrun-vcs` REST client so both speak to the providers identically.
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

/// Normalize a provider's list-repos JSON (a top-level array) into [`Repo`]s.
fn parse_repos(provider: Provider, value: &serde_json::Value) -> Vec<Repo> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| parse_repo(provider, item))
        .collect()
}

/// Normalize a single repository object, or `None` if it lacks a name.
fn parse_repo(provider: Provider, item: &serde_json::Value) -> Option<Repo> {
    let str_field = |key: &str| item.get(key).and_then(|v| v.as_str()).map(str::to_string);
    match provider {
        Provider::GitHub => {
            let name = str_field("name")?;
            Some(Repo {
                full_name: str_field("full_name").unwrap_or_else(|| name.clone()),
                url: str_field("html_url").unwrap_or_default(),
                name,
                provider,
            })
        }
        Provider::GitLab => {
            let name = str_field("name")?;
            Some(Repo {
                full_name: str_field("path_with_namespace").unwrap_or_else(|| name.clone()),
                url: str_field("web_url").unwrap_or_default(),
                name,
                provider,
            })
        }
    }
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
    fn github_repos_normalize() {
        let body = serde_json::json!([
            { "name": "darkrun", "full_name": "jwaldrip/darkrun", "html_url": "https://github.com/jwaldrip/darkrun" },
            { "name": "other", "full_name": "acme/other", "html_url": "https://github.com/acme/other" },
        ]);
        let repos = parse_repos(Provider::GitHub, &body);
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].name, "darkrun");
        assert_eq!(repos[0].full_name, "jwaldrip/darkrun");
        assert_eq!(repos[0].url, "https://github.com/jwaldrip/darkrun");
        assert_eq!(repos[0].provider, Provider::GitHub);
    }

    #[test]
    fn gitlab_repos_normalize() {
        let body = serde_json::json!([
            { "name": "darkrun", "path_with_namespace": "jwaldrip/darkrun", "web_url": "https://gitlab.com/jwaldrip/darkrun" },
        ]);
        let repos = parse_repos(Provider::GitLab, &body);
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].full_name, "jwaldrip/darkrun");
        assert_eq!(repos[0].url, "https://gitlab.com/jwaldrip/darkrun");
        assert_eq!(repos[0].provider, Provider::GitLab);
    }

    #[test]
    fn repos_without_a_name_are_skipped() {
        let body = serde_json::json!([{ "full_name": "no/name" }, { "name": "ok" }]);
        let repos = parse_repos(Provider::GitHub, &body);
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "ok");
    }

    #[test]
    fn non_array_payload_yields_no_repos() {
        let body = serde_json::json!({ "message": "Bad credentials" });
        assert!(parse_repos(Provider::GitHub, &body).is_empty());
    }

    #[test]
    fn repos_url_is_provider_specific() {
        assert_eq!(
            repos_url(Provider::GitHub),
            "https://api.github.com/user/repos?per_page=100&sort=updated"
        );
        assert_eq!(
            repos_url(Provider::GitLab),
            "https://gitlab.com/api/v4/projects?membership=true&per_page=100&order_by=last_activity_at"
        );
    }

    #[test]
    fn bearer_token_is_extracted_case_insensitively() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(axum::http::header::AUTHORIZATION, "Bearer abc123".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("abc123".to_string()));
        headers.insert(axum::http::header::AUTHORIZATION, "bearer xyz".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("xyz".to_string()));
        // No header → None.
        assert_eq!(bearer_token(&axum::http::HeaderMap::new()), None);
    }

    // ---- multi-page fetch (offline via the mock transport) ------------------

    /// The list-repos URL for a page — `repos_url` plus `&page=N`, exactly as
    /// [`fetch_repos`] builds it, so the mock keys line up.
    fn repos_page_url(provider: Provider, page: u32) -> String {
        format!("{}&page={}", repos_url(provider), page)
    }

    /// A GitLab `/projects` page body: a bare array of project objects.
    fn gitlab_projects_body(names: &[String]) -> Vec<u8> {
        let arr: Vec<serde_json::Value> = names
            .iter()
            .map(|n| {
                serde_json::json!({
                    "name": n,
                    "path_with_namespace": format!("jwaldrip/{n}"),
                    "web_url": format!("https://gitlab.com/jwaldrip/{n}"),
                })
            })
            .collect();
        serde_json::to_vec(&serde_json::Value::Array(arr)).unwrap()
    }

    #[test]
    fn gitlab_projects_are_walked_across_pages() {
        let cred = Credential::new(Provider::GitLab, "glpat");
        let mock = MockTransport::new();
        // A full first page (100 projects) forces page 2; the short second page
        // (1 project) ends pagination.
        let page1: Vec<String> = (0..REPOS_PER_PAGE).map(|i| format!("p-{i}")).collect();
        mock.expect(
            Method::Get,
            repos_page_url(Provider::GitLab, 1),
            HttpResponse::new(200, gitlab_projects_body(&page1)),
        );
        mock.expect(
            Method::Get,
            repos_page_url(Provider::GitLab, 2),
            HttpResponse::new(200, gitlab_projects_body(&["last".to_string()])),
        );
        let repos = fetch_repos(&mock, Provider::GitLab, &cred).unwrap();
        // Both pages are concatenated.
        assert_eq!(repos.len(), REPOS_PER_PAGE + 1);
        assert_eq!(repos[0].full_name, "jwaldrip/p-0");
        assert_eq!(repos.last().unwrap().name, "last");
        // Exactly two pages fetched (the short second page stops paging).
        assert_eq!(mock.requests().len(), 2);
    }

    #[test]
    fn gitlab_projects_single_short_page_makes_one_request() {
        let cred = Credential::new(Provider::GitLab, "glpat");
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            repos_page_url(Provider::GitLab, 1),
            HttpResponse::new(200, gitlab_projects_body(&["a".to_string(), "b".to_string()])),
        );
        let repos = fetch_repos(&mock, Provider::GitLab, &cred).unwrap();
        assert_eq!(repos.len(), 2);
        // A short first page ends pagination immediately — exactly one request.
        assert_eq!(mock.requests().len(), 1);
    }

    #[test]
    fn github_repos_are_walked_across_pages() {
        // GitHub `/user/repos` is the same bare-array + `?page=N` model, so it is
        // walked identically.
        let cred = Credential::new(Provider::GitHub, "gho");
        let mock = MockTransport::new();
        let page1: Vec<serde_json::Value> = (0..REPOS_PER_PAGE)
            .map(|i| {
                serde_json::json!({
                    "name": format!("g-{i}"),
                    "full_name": format!("acme/g-{i}"),
                    "html_url": format!("https://github.com/acme/g-{i}"),
                })
            })
            .collect();
        mock.expect(
            Method::Get,
            repos_page_url(Provider::GitHub, 1),
            HttpResponse::new(200, serde_json::to_vec(&serde_json::Value::Array(page1)).unwrap()),
        );
        mock.expect(
            Method::Get,
            repos_page_url(Provider::GitHub, 2),
            HttpResponse::new(
                200,
                serde_json::to_vec(&serde_json::json!([
                    { "name": "tail", "full_name": "acme/tail", "html_url": "https://github.com/acme/tail" }
                ]))
                .unwrap(),
            ),
        );
        let repos = fetch_repos(&mock, Provider::GitHub, &cred).unwrap();
        assert_eq!(repos.len(), REPOS_PER_PAGE + 1);
        assert_eq!(repos.last().unwrap().name, "tail");
        assert_eq!(mock.requests().len(), 2);
    }
}
