//! Detect whether a REMOTE repo already carries darkrun artifacts, without
//! cloning it.
//!
//! The desktop catalog's source of truth is the provider (the GitHub/GitLab
//! repos you can access), and it auto-adds a repo when darkrun has already run
//! against it. "Has run against it" is detectable remotely in two ways, either
//! of which counts:
//!
//! 1. a branch under the `darkrun/` namespace (`darkrun/<slug>/main`,
//!    `darkrun/<slug>/<station>`, ...), or
//! 2. a committed `.darkrun/` directory on the repo's default branch.
//!
//! Each check is one provider API call (no clone, no fetch). A branch match
//! short-circuits, so the common case is a single request. Auth + accept headers
//! reuse the same [`crate::rest::authorize`] path as the PR/MR clients.

use crate::error::Result;
use crate::oauth::percent_encode;
use crate::provider::{Credential, Provider};
use crate::remote::RepoCoords;
use crate::rest::{api_error, authorize};
use crate::transport::{HttpRequest, HttpResponse, HttpTransport};

/// The branch prefix darkrun creates every run branch under. Mirrors
/// `darkrun_mcp::lifecycle::BRANCH_PREFIX` + `/`; duplicated here so darkrun-vcs
/// stays free of an engine-crate dependency.
const DARKRUN_BRANCH_PREFIX: &str = "darkrun/";

/// The tracked directory darkrun commits run state into on the default branch.
const DARKRUN_DIR: &str = ".darkrun";

/// Whether `coords` (on `provider`) already carries darkrun artifacts: a
/// `darkrun/*` branch, or a `.darkrun/` directory on the default branch.
///
/// One or two API calls; a branch hit short-circuits the directory check. A
/// `404` (empty repo, missing path/ref) reads as "no artifacts", not an error;
/// any other non-2xx surfaces as [`crate::VcsError::Api`] so a transient failure
/// is distinguishable from a definite "no" by the caller.
pub fn remote_has_darkrun_artifacts(
    transport: &dyn HttpTransport,
    provider: Provider,
    cred: &Credential,
    coords: &RepoCoords,
) -> Result<bool> {
    if has_darkrun_branch(transport, provider, cred, coords)? {
        return Ok(true);
    }
    has_darkrun_dir(transport, provider, cred, coords)
}

/// Whether the repo has any branch under the `darkrun/` namespace.
fn has_darkrun_branch(
    transport: &dyn HttpTransport,
    provider: Provider,
    cred: &Credential,
    coords: &RepoCoords,
) -> Result<bool> {
    match provider {
        Provider::GitHub => {
            // `git/matching-refs/heads/darkrun/` returns every ref whose name
            // starts with the prefix: a non-empty array means a darkrun branch
            // exists. Empty repos 409/404 here, which we read as "no".
            let url = format!(
                "{base}/repos/{owner}/{repo}/git/matching-refs/heads/{prefix}",
                base = Provider::GitHub.api_base(),
                owner = coords.owner,
                repo = coords.repo,
                prefix = DARKRUN_BRANCH_PREFIX,
            );
            let value = match get_json(transport, provider, cred, url)? {
                Some(v) => v,
                None => return Ok(false),
            };
            Ok(value.as_array().is_some_and(|refs| !refs.is_empty()))
        }
        Provider::GitLab => {
            // GitLab branch search matches a substring; `^darkrun/` anchors it to
            // the start on supporting versions, and we re-check the prefix
            // client-side so an older server that ignores the anchor can't yield
            // a false positive.
            let url = format!(
                "{base}/projects/{id}/repository/branches?search={q}",
                base = Provider::GitLab.api_base(),
                id = percent_encode(&coords.project_path()),
                q = percent_encode(&format!("^{DARKRUN_BRANCH_PREFIX}")),
            );
            let value = match get_json(transport, provider, cred, url)? {
                Some(v) => v,
                None => return Ok(false),
            };
            Ok(value.as_array().is_some_and(|branches| {
                branches.iter().any(|b| {
                    b.get("name")
                        .and_then(|n| n.as_str())
                        .is_some_and(|n| n.starts_with(DARKRUN_BRANCH_PREFIX))
                })
            }))
        }
    }
}

/// Whether the repo has a `.darkrun/` directory on its default branch.
fn has_darkrun_dir(
    transport: &dyn HttpTransport,
    provider: Provider,
    cred: &Credential,
    coords: &RepoCoords,
) -> Result<bool> {
    match provider {
        Provider::GitHub => {
            // Omitting `?ref=` targets the default branch. A directory returns a
            // 200 JSON array of entries; a missing path is a 404 ("no").
            let url = format!(
                "{base}/repos/{owner}/{repo}/contents/{dir}",
                base = Provider::GitHub.api_base(),
                owner = coords.owner,
                repo = coords.repo,
                dir = DARKRUN_DIR,
            );
            Ok(get_json(transport, provider, cred, url)?.is_some())
        }
        Provider::GitLab => {
            // `repository/tree?path=.darkrun` (default ref) lists the directory's
            // entries: a non-empty array means the directory exists. A missing
            // path is a 404 ("no").
            let url = format!(
                "{base}/projects/{id}/repository/tree?path={dir}",
                base = Provider::GitLab.api_base(),
                id = percent_encode(&coords.project_path()),
                dir = percent_encode(DARKRUN_DIR),
            );
            let value = match get_json(transport, provider, cred, url)? {
                Some(v) => v,
                None => return Ok(false),
            };
            Ok(value.as_array().is_some_and(|entries| !entries.is_empty()))
        }
    }
}

/// Issue an authorized `GET` and decode the body as JSON. Returns `Ok(None)` for
/// a `404` (the resource does not exist — a definite "no", not a failure), the
/// decoded value for a `2xx`, and an error for any other status.
fn get_json(
    transport: &dyn HttpTransport,
    provider: Provider,
    cred: &Credential,
    url: String,
) -> Result<Option<serde_json::Value>> {
    let request = authorize(HttpRequest::get(url), provider, cred);
    let response: HttpResponse = transport.execute(request)?;
    if response.status == 404 {
        return Ok(None);
    }
    if !response.is_success() {
        return Err(api_error(provider, &response));
    }
    Ok(Some(response.json()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{Method, MockTransport};

    fn cred(provider: Provider) -> Credential {
        Credential {
            provider,
            access_token: "t".into(),
            refresh_token: None,
            expires_in: None,
            token_type: None,
        }
    }

    fn ok(body: &str) -> HttpResponse {
        HttpResponse::new(200, body.as_bytes().to_vec())
    }

    fn not_found() -> HttpResponse {
        HttpResponse::new(404, br#"{"message":"Not Found"}"#.to_vec())
    }

    // ---- GitHub ----

    #[test]
    fn github_darkrun_branch_short_circuits_before_the_dir_check() {
        let coords = RepoCoords::new("github.com", "acme", "widgets");
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            "https://api.github.com/repos/acme/widgets/git/matching-refs/heads/darkrun/",
            ok(r#"[{"ref":"refs/heads/darkrun/foo/main"}]"#),
        );
        assert!(remote_has_darkrun_artifacts(&mock, Provider::GitHub, &cred(Provider::GitHub), &coords).unwrap());
        // Only the branch call happened; the contents call was never reached.
        assert_eq!(mock.requests().len(), 1);
    }

    #[test]
    fn github_no_branch_then_darkrun_dir_present() {
        let coords = RepoCoords::new("github.com", "acme", "widgets");
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            "https://api.github.com/repos/acme/widgets/git/matching-refs/heads/darkrun/",
            ok("[]"),
        );
        mock.expect(
            Method::Get,
            "https://api.github.com/repos/acme/widgets/contents/.darkrun",
            ok(r#"[{"name":"run-1","type":"dir"}]"#),
        );
        assert!(remote_has_darkrun_artifacts(&mock, Provider::GitHub, &cred(Provider::GitHub), &coords).unwrap());
        assert_eq!(mock.requests().len(), 2);
    }

    #[test]
    fn github_no_branch_no_dir_is_false() {
        let coords = RepoCoords::new("github.com", "acme", "widgets");
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            "https://api.github.com/repos/acme/widgets/git/matching-refs/heads/darkrun/",
            ok("[]"),
        );
        mock.expect(
            Method::Get,
            "https://api.github.com/repos/acme/widgets/contents/.darkrun",
            not_found(),
        );
        assert!(!remote_has_darkrun_artifacts(&mock, Provider::GitHub, &cred(Provider::GitHub), &coords).unwrap());
    }

    // ---- GitLab ----

    #[test]
    fn gitlab_darkrun_branch_matched_by_prefix() {
        let coords = RepoCoords::new("gitlab.com", "grp/sub", "widgets");
        let mock = MockTransport::new();
        // project path url-encoded: grp%2Fsub%2Fwidgets; search ^darkrun/ encoded.
        mock.expect(
            Method::Get,
            "https://gitlab.com/api/v4/projects/grp%2Fsub%2Fwidgets/repository/branches?search=%5Edarkrun%2F",
            ok(r#"[{"name":"darkrun/abc/main"}]"#),
        );
        assert!(remote_has_darkrun_artifacts(&mock, Provider::GitLab, &cred(Provider::GitLab), &coords).unwrap());
    }

    #[test]
    fn gitlab_substring_match_without_prefix_is_rejected() {
        // An old server that treats `^darkrun/` as a plain substring could return
        // a branch like `feature/darkrun/x`; the client-side prefix guard rejects it.
        let coords = RepoCoords::new("gitlab.com", "grp", "widgets");
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            "https://gitlab.com/api/v4/projects/grp%2Fwidgets/repository/branches?search=%5Edarkrun%2F",
            ok(r#"[{"name":"feature/darkrun/x"}]"#),
        );
        mock.expect(
            Method::Get,
            "https://gitlab.com/api/v4/projects/grp%2Fwidgets/repository/tree?path=.darkrun",
            not_found(),
        );
        assert!(!remote_has_darkrun_artifacts(&mock, Provider::GitLab, &cred(Provider::GitLab), &coords).unwrap());
    }

    #[test]
    fn gitlab_darkrun_dir_present() {
        let coords = RepoCoords::new("gitlab.com", "grp", "widgets");
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            "https://gitlab.com/api/v4/projects/grp%2Fwidgets/repository/branches?search=%5Edarkrun%2F",
            ok("[]"),
        );
        mock.expect(
            Method::Get,
            "https://gitlab.com/api/v4/projects/grp%2Fwidgets/repository/tree?path=.darkrun",
            ok(r#"[{"name":"state.json"}]"#),
        );
        assert!(remote_has_darkrun_artifacts(&mock, Provider::GitLab, &cred(Provider::GitLab), &coords).unwrap());
    }

    #[test]
    fn gitlab_empty_tree_is_false() {
        let coords = RepoCoords::new("gitlab.com", "grp", "widgets");
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            "https://gitlab.com/api/v4/projects/grp%2Fwidgets/repository/branches?search=%5Edarkrun%2F",
            ok("[]"),
        );
        mock.expect(
            Method::Get,
            "https://gitlab.com/api/v4/projects/grp%2Fwidgets/repository/tree?path=.darkrun",
            ok("[]"),
        );
        assert!(!remote_has_darkrun_artifacts(&mock, Provider::GitLab, &cred(Provider::GitLab), &coords).unwrap());
    }

    #[test]
    fn a_non_404_error_surfaces_rather_than_reading_as_no() {
        let coords = RepoCoords::new("github.com", "acme", "widgets");
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            "https://api.github.com/repos/acme/widgets/git/matching-refs/heads/darkrun/",
            HttpResponse::new(500, br#"{"message":"boom"}"#.to_vec()),
        );
        assert!(remote_has_darkrun_artifacts(&mock, Provider::GitHub, &cred(Provider::GitHub), &coords).is_err());
    }
}
