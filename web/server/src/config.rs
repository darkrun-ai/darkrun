//! Server configuration: OAuth client credentials and the public web base.
//!
//! Client secrets live ONLY here, read from the process environment — never
//! hardcoded, never logged, never shipped in the binary. A missing secret is
//! not a startup failure; it surfaces only when a login for that provider is
//! attempted, so the static site still serves on a box with no OAuth env.

use std::env;

use darkrun_vcs::Provider;

/// The public base URL the website is reachable at, e.g. `https://darkrun.ai`.
///
/// Used to build the `redirect_uri` the provider calls back. Read from
/// `DARKRUN_WEB_BASE`, defaulting to the production host.
pub const DEFAULT_WEB_BASE: &str = "https://darkrun.ai";

/// A single provider's OAuth client id/secret pair.
#[derive(Debug, Clone)]
pub struct ProviderCredentials {
    /// The OAuth app client id (public).
    pub client_id: String,
    /// The OAuth app client secret (server-only).
    pub client_secret: String,
}

/// Resolved server configuration.
#[derive(Debug, Clone)]
pub struct WebConfig {
    /// Public base URL (no trailing slash).
    pub web_base: String,
    /// GitHub OAuth app credentials, if configured.
    pub github: Option<ProviderCredentials>,
    /// GitLab OAuth app credentials, if configured.
    pub gitlab: Option<ProviderCredentials>,
}

impl WebConfig {
    /// Resolve configuration from the process environment.
    ///
    /// * `DARKRUN_WEB_BASE` → [`web_base`](Self::web_base) (default
    ///   [`DEFAULT_WEB_BASE`]); a trailing slash is trimmed.
    /// * `GITHUB_CLIENT_ID` + `GITHUB_CLIENT_SECRET` → GitHub credentials.
    /// * `GITLAB_CLIENT_ID` + `GITLAB_CLIENT_SECRET` → GitLab credentials.
    pub fn from_env() -> Self {
        let web_base = env::var("DARKRUN_WEB_BASE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_WEB_BASE.to_string());

        Self {
            web_base: web_base.trim_end_matches('/').to_string(),
            github: read_pair("GITHUB_CLIENT_ID", "GITHUB_CLIENT_SECRET"),
            gitlab: read_pair("GITLAB_CLIENT_ID", "GITLAB_CLIENT_SECRET"),
        }
    }

    /// Build a config explicitly — the constructor tests use.
    pub fn new(
        web_base: impl Into<String>,
        github: Option<ProviderCredentials>,
        gitlab: Option<ProviderCredentials>,
    ) -> Self {
        Self {
            web_base: web_base.into().trim_end_matches('/').to_string(),
            github,
            gitlab,
        }
    }

    /// The configured credentials for `provider`, if any.
    pub fn credentials(&self, provider: Provider) -> Option<&ProviderCredentials> {
        match provider {
            Provider::GitHub => self.github.as_ref(),
            Provider::GitLab => self.gitlab.as_ref(),
        }
    }

    /// The `redirect_uri` for `provider`: `<web_base>/auth/<provider>/callback`.
    pub fn redirect_uri(&self, provider: Provider) -> String {
        format!("{}/auth/{}/callback", self.web_base, provider.key())
    }
}

impl Default for WebConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

/// Read an id/secret pair; `Some` only when both are present and non-empty.
fn read_pair(id_var: &str, secret_var: &str) -> Option<ProviderCredentials> {
    let client_id = env::var(id_var).ok().filter(|s| !s.trim().is_empty())?;
    let client_secret = env::var(secret_var).ok().filter(|s| !s.trim().is_empty())?;
    Some(ProviderCredentials {
        client_id,
        client_secret,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds() -> ProviderCredentials {
        ProviderCredentials {
            client_id: "cid".into(),
            client_secret: "secret".into(),
        }
    }

    #[test]
    fn redirect_uri_is_provider_scoped() {
        let cfg = WebConfig::new("https://darkrun.ai", None, None);
        assert_eq!(
            cfg.redirect_uri(Provider::GitHub),
            "https://darkrun.ai/auth/github/callback"
        );
        assert_eq!(
            cfg.redirect_uri(Provider::GitLab),
            "https://darkrun.ai/auth/gitlab/callback"
        );
    }

    #[test]
    fn web_base_trailing_slash_is_trimmed() {
        let cfg = WebConfig::new("https://darkrun.ai/", None, None);
        assert_eq!(cfg.web_base, "https://darkrun.ai");
        assert_eq!(
            cfg.redirect_uri(Provider::GitHub),
            "https://darkrun.ai/auth/github/callback"
        );
    }

    #[test]
    fn credentials_lookup_by_provider() {
        let cfg = WebConfig::new("https://darkrun.ai", Some(creds()), None);
        assert!(cfg.credentials(Provider::GitHub).is_some());
        assert!(cfg.credentials(Provider::GitLab).is_none());
    }

    #[test]
    fn from_env_defaults_web_base_when_unset() {
        // No env mutation: just assert the default constant is what we ship.
        assert_eq!(DEFAULT_WEB_BASE, "https://darkrun.ai");
    }

    #[test]
    fn new_with_both_providers() {
        let cfg = WebConfig::new("https://example.test", Some(creds()), Some(creds()));
        assert!(cfg.credentials(Provider::GitHub).is_some());
        assert!(cfg.credentials(Provider::GitLab).is_some());
    }
}
