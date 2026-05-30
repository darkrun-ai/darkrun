//! Git remote URL → repo coordinate parsing tests.

use darkrun_vcs::{parse_remote_url, Provider, VcsError};

#[test]
fn parses_github_https_with_dot_git() {
    let c = parse_remote_url("https://github.com/jwaldrip/darkrun.git").unwrap();
    assert_eq!(c.host, "github.com");
    assert_eq!(c.owner, "jwaldrip");
    assert_eq!(c.repo, "darkrun");
    assert_eq!(c.slug(), "jwaldrip/darkrun");
    assert_eq!(c.provider(), Some(Provider::GitHub));
}

#[test]
fn parses_github_https_without_dot_git() {
    let c = parse_remote_url("https://github.com/owner/repo").unwrap();
    assert_eq!(c.repo, "repo");
    assert_eq!(c.owner, "owner");
}

#[test]
fn parses_github_scp_ssh() {
    let c = parse_remote_url("git@github.com:jwaldrip/darkrun.git").unwrap();
    assert_eq!(c.host, "github.com");
    assert_eq!(c.owner, "jwaldrip");
    assert_eq!(c.repo, "darkrun");
}

#[test]
fn parses_ssh_url_form() {
    let c = parse_remote_url("ssh://git@github.com/owner/repo.git").unwrap();
    assert_eq!(c.host, "github.com");
    assert_eq!(c.owner, "owner");
    assert_eq!(c.repo, "repo");
}

#[test]
fn parses_gitlab_https() {
    let c = parse_remote_url("https://gitlab.com/group/project.git").unwrap();
    assert_eq!(c.host, "gitlab.com");
    assert_eq!(c.owner, "group");
    assert_eq!(c.repo, "project");
    assert_eq!(c.provider(), Some(Provider::GitLab));
}

#[test]
fn parses_gitlab_subgroups_https() {
    let c = parse_remote_url("https://gitlab.com/group/subgroup/project.git").unwrap();
    assert_eq!(c.host, "gitlab.com");
    assert_eq!(c.owner, "group/subgroup");
    assert_eq!(c.repo, "project");
    assert_eq!(c.project_path(), "group/subgroup/project");
}

#[test]
fn parses_gitlab_subgroups_scp() {
    let c = parse_remote_url("git@gitlab.com:group/sub1/sub2/project.git").unwrap();
    assert_eq!(c.owner, "group/sub1/sub2");
    assert_eq!(c.repo, "project");
}

#[test]
fn parses_https_with_port() {
    let c = parse_remote_url("https://git.example.com:8443/owner/repo.git").unwrap();
    assert_eq!(c.host, "git.example.com");
    assert_eq!(c.owner, "owner");
    assert_eq!(c.repo, "repo");
}

#[test]
fn rejects_empty() {
    assert!(matches!(
        parse_remote_url("   "),
        Err(VcsError::RemoteParse(_))
    ));
}

#[test]
fn rejects_missing_owner() {
    assert!(matches!(
        parse_remote_url("https://github.com/justrepo"),
        Err(VcsError::RemoteParse(_))
    ));
}

#[test]
fn rejects_garbage() {
    assert!(matches!(
        parse_remote_url("not-a-url"),
        Err(VcsError::RemoteParse(_))
    ));
}

#[test]
fn trailing_slash_is_tolerated() {
    let c = parse_remote_url("https://github.com/owner/repo/").unwrap();
    assert_eq!(c.repo, "repo");
}
