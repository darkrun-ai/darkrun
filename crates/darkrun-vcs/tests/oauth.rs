//! OAuth authorize-URL building and code→token exchange tests.

use darkrun_vcs::transport::{HttpResponse, Method};
use darkrun_vcs::{
    authorize_url, exchange_code, percent_encode, MockTransport, Provider, VcsError,
};

#[test]
fn authorize_url_github_has_all_params_and_repo_scope() {
    let url = authorize_url(
        Provider::GitHub,
        "client-123",
        "https://darkrun.ai/auth/github/callback",
        "nonce-abc",
    );
    assert!(url.starts_with("https://github.com/login/oauth/authorize?"));
    assert!(url.contains("client_id=client-123"));
    assert!(url.contains("scope=repo"));
    assert!(url.contains("state=nonce-abc"));
    assert!(url.contains("response_type=code"));
    // redirect_uri must be percent-encoded.
    assert!(url.contains("redirect_uri=https%3A%2F%2Fdarkrun.ai%2Fauth%2Fgithub%2Fcallback"));
}

#[test]
fn authorize_url_gitlab_uses_api_scope_and_gitlab_endpoint() {
    let url = authorize_url(
        Provider::GitLab,
        "gl-client",
        "https://darkrun.ai/auth/gitlab/callback",
        "state-xyz",
    );
    assert!(url.starts_with("https://gitlab.com/oauth/authorize?"));
    assert!(url.contains("scope=api"));
    assert!(url.contains("client_id=gl-client"));
}

#[test]
fn percent_encode_leaves_unreserved_and_escapes_the_rest() {
    assert_eq!(percent_encode("aZ09-_.~"), "aZ09-_.~");
    assert_eq!(percent_encode("a b/c?d=e&f"), "a%20b%2Fc%3Fd%3De%26f");
    assert_eq!(percent_encode("state with space"), "state%20with%20space");
}

#[test]
fn exchange_code_github_success_parses_token() {
    let mock = MockTransport::new();
    mock.expect(
        Method::Post,
        "https://github.com/login/oauth/access_token",
        HttpResponse::new(
            200,
            r#"{"access_token":"gho_abc","token_type":"bearer","scope":"repo"}"#,
        ),
    );

    let cred = exchange_code(
        &mock,
        Provider::GitHub,
        "id",
        "secret",
        "the-code",
        "https://darkrun.ai/auth/github/callback",
    )
    .expect("exchange should succeed");

    assert_eq!(cred.provider, Provider::GitHub);
    assert_eq!(cred.access_token, "gho_abc");
    assert_eq!(cred.token_type.as_deref(), Some("bearer"));

    // The request must carry the secret in the body and request JSON back.
    let req = mock.single_request();
    assert_eq!(req.method, Method::Post);
    let body: serde_json::Value =
        serde_json::from_slice(req.body.as_ref().expect("body")).expect("json body");
    assert_eq!(body["client_secret"], "secret");
    assert_eq!(body["code"], "the-code");
    assert_eq!(body["grant_type"], "authorization_code");
    assert!(req
        .headers
        .iter()
        .any(|(k, v)| k == "Accept" && v == "application/json"));
}

#[test]
fn exchange_code_gitlab_success_includes_refresh_and_expiry() {
    let mock = MockTransport::new();
    mock.expect(
        Method::Post,
        "https://gitlab.com/oauth/token",
        HttpResponse::new(
            200,
            r#"{"access_token":"glpat","refresh_token":"r1","expires_in":7200,"token_type":"bearer"}"#,
        ),
    );

    let cred = exchange_code(
        &mock,
        Provider::GitLab,
        "id",
        "secret",
        "code",
        "https://darkrun.ai/auth/gitlab/callback",
    )
    .expect("exchange should succeed");

    assert_eq!(cred.access_token, "glpat");
    assert_eq!(cred.refresh_token.as_deref(), Some("r1"));
    assert_eq!(cred.expires_in, Some(7200));
}

#[test]
fn exchange_code_error_json_with_200_status_is_surfaced() {
    // GitHub returns a 200 with an `error` body for bad codes.
    let mock = MockTransport::new();
    mock.expect(
        Method::Post,
        "https://github.com/login/oauth/access_token",
        HttpResponse::new(
            200,
            r#"{"error":"bad_verification_code","error_description":"The code passed is incorrect."}"#,
        ),
    );

    let err = exchange_code(
        &mock,
        Provider::GitHub,
        "id",
        "secret",
        "nope",
        "https://darkrun.ai/auth/github/callback",
    )
    .expect_err("should surface oauth error");

    match err {
        VcsError::OauthExchange { error, description } => {
            assert_eq!(error, "bad_verification_code");
            assert_eq!(description.as_deref(), Some("The code passed is incorrect."));
        }
        other => panic!("expected OauthExchange, got {other:?}"),
    }
}

#[test]
fn exchange_code_missing_access_token_errors() {
    let mock = MockTransport::new();
    mock.expect(
        Method::Post,
        "https://github.com/login/oauth/access_token",
        HttpResponse::new(200, r#"{"token_type":"bearer"}"#),
    );

    let err = exchange_code(
        &mock,
        Provider::GitHub,
        "id",
        "secret",
        "code",
        "https://darkrun.ai/auth/github/callback",
    )
    .expect_err("missing token should error");

    assert!(matches!(err, VcsError::MissingField("access_token")));
}

#[test]
fn exchange_code_transport_error_when_no_mock_queued() {
    let mock = MockTransport::new();
    let err = exchange_code(
        &mock,
        Provider::GitLab,
        "id",
        "secret",
        "code",
        "https://darkrun.ai/auth/gitlab/callback",
    )
    .expect_err("no queued response");
    assert!(matches!(err, VcsError::Transport(_)));
}
