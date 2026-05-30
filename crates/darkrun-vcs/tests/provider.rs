//! Provider enum + credential model + transport helper tests.

use darkrun_vcs::transport::{HttpRequest, HttpResponse, Method, MockTransport};
use darkrun_vcs::transport::HttpTransport;
use darkrun_vcs::{Credential, Provider};

#[test]
fn provider_keys_and_scopes() {
    assert_eq!(Provider::GitHub.key(), "github");
    assert_eq!(Provider::GitLab.key(), "gitlab");
    assert_eq!(Provider::GitHub.oauth_scope(), "repo");
    assert_eq!(Provider::GitLab.oauth_scope(), "api");
    assert_eq!(Provider::GitHub.display_name(), "GitHub");
    assert_eq!(Provider::GitLab.display_name(), "GitLab");
}

#[test]
fn provider_endpoints() {
    assert_eq!(Provider::GitHub.api_base(), "https://api.github.com");
    assert_eq!(Provider::GitLab.api_base(), "https://gitlab.com/api/v4");
    assert!(Provider::GitHub.token_endpoint().contains("access_token"));
    assert!(Provider::GitLab.token_endpoint().contains("/oauth/token"));
}

#[test]
fn provider_from_key_aliases() {
    assert_eq!(Provider::from_key("github"), Some(Provider::GitHub));
    assert_eq!(Provider::from_key("gh"), Some(Provider::GitHub));
    assert_eq!(Provider::from_key("GitLab"), Some(Provider::GitLab));
    assert_eq!(Provider::from_key("gl"), Some(Provider::GitLab));
    assert_eq!(Provider::from_key("bitbucket"), None);
}

#[test]
fn provider_from_host() {
    assert_eq!(Provider::from_host("github.com"), Some(Provider::GitHub));
    assert_eq!(Provider::from_host("gitlab.com"), Some(Provider::GitLab));
    assert_eq!(
        Provider::from_host("gitlab.example.org"),
        Some(Provider::GitLab)
    );
    assert_eq!(Provider::from_host("example.com"), None);
}

#[test]
fn provider_serde_roundtrip_lowercase() {
    let json = serde_json::to_string(&Provider::GitHub).unwrap();
    assert_eq!(json, "\"github\"");
    let back: Provider = serde_json::from_str("\"gitlab\"").unwrap();
    assert_eq!(back, Provider::GitLab);
}

#[test]
fn credential_authorization_header() {
    let c = Credential::new(Provider::GitHub, "abc");
    assert_eq!(c.authorization_header(), "Bearer abc");
}

#[test]
fn credential_serde_skips_none_optionals() {
    let c = Credential::new(Provider::GitHub, "t");
    let json = serde_json::to_string(&c).unwrap();
    assert!(!json.contains("refresh_token"));
    assert!(!json.contains("expires_in"));
    assert!(json.contains("\"access_token\":\"t\""));
}

#[test]
fn mock_transport_fifo_per_key() {
    let mock = MockTransport::new();
    mock.expect(Method::Get, "https://x/a", HttpResponse::new(200, "first"));
    mock.expect(Method::Get, "https://x/a", HttpResponse::new(200, "second"));

    let r1 = mock.execute(HttpRequest::get("https://x/a")).unwrap();
    let r2 = mock.execute(HttpRequest::get("https://x/a")).unwrap();
    assert_eq!(r1.text().unwrap(), "first");
    assert_eq!(r2.text().unwrap(), "second");
}

#[test]
fn http_response_helpers() {
    let ok = HttpResponse::new(204, "");
    assert!(ok.is_success());
    let bad = HttpResponse::new(500, "boom");
    assert!(!bad.is_success());
    assert_eq!(bad.text().unwrap(), "boom");

    let json = HttpResponse::new(200, r#"{"n":5}"#);
    let v: serde_json::Value = json.json().unwrap();
    assert_eq!(v["n"], 5);
}

#[test]
fn http_request_builders() {
    let req = HttpRequest::post("https://x/p")
        .header("Accept", "application/json")
        .json_body(&serde_json::json!({"a": 1}))
        .unwrap();
    assert_eq!(req.method, Method::Post);
    assert!(req
        .headers
        .iter()
        .any(|(k, v)| k == "Content-Type" && v == "application/json"));
    assert!(req.body.is_some());
}
