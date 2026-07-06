//! The [`Provider`] enum, [`Credential`] model, and provider-specific OAuth and
//! API endpoint constants.

use serde::{Deserialize, Serialize};

/// A supported version-control provider.
///
/// GitHub speaks Pull Requests; GitLab speaks Merge Requests. The unified
/// change-request API normalizes over the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// github.com (Pull Requests).
    GitHub,
    /// gitlab.com (Merge Requests).
    GitLab,
}

impl Provider {
    /// The lowercase wire/storage key for this provider.
    pub fn key(self) -> &'static str {
        match self {
            Provider::GitHub => "github",
            Provider::GitLab => "gitlab",
        }
    }

    /// A human-facing display name.
    pub fn display_name(self) -> &'static str {
        match self {
            Provider::GitHub => "GitHub",
            Provider::GitLab => "GitLab",
        }
    }

    /// The default web host for this provider.
    pub fn default_host(self) -> &'static str {
        match self {
            Provider::GitHub => "github.com",
            Provider::GitLab => "gitlab.com",
        }
    }

    /// The OAuth scope requested for this provider.
    ///
    /// GitHub uses `repo` (full repo access incl. PRs); GitLab uses `api`.
    pub fn oauth_scope(self) -> &'static str {
        match self {
            Provider::GitHub => "repo",
            Provider::GitLab => "api",
        }
    }

    /// The provider's OAuth authorize endpoint.
    pub fn authorize_endpoint(self) -> &'static str {
        match self {
            Provider::GitHub => "https://github.com/login/oauth/authorize",
            Provider::GitLab => "https://gitlab.com/oauth/authorize",
        }
    }

    /// The provider's OAuth token-exchange endpoint.
    pub fn token_endpoint(self) -> &'static str {
        match self {
            Provider::GitHub => "https://github.com/login/oauth/access_token",
            Provider::GitLab => "https://gitlab.com/oauth/token",
        }
    }

    /// The provider's REST API base URL.
    pub fn api_base(self) -> &'static str {
        match self {
            Provider::GitHub => "https://api.github.com",
            Provider::GitLab => "https://gitlab.com/api/v4",
        }
    }

    /// Parse a provider from its lowercase key. Accepts a few aliases.
    pub fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "github" | "gh" => Some(Provider::GitHub),
            "gitlab" | "gl" => Some(Provider::GitLab),
            _ => None,
        }
    }

    /// Infer the provider from a host (e.g. `github.com`, `gitlab.com`).
    ///
    /// Only the two providers' canonical SaaS hosts are recognized. A
    /// self-hosted instance is deliberately NOT inferred: this crate's REST and
    /// OAuth endpoints ([`api_base`](Self::api_base),
    /// [`authorize_endpoint`](Self::authorize_endpoint),
    /// [`token_endpoint`](Self::token_endpoint)) are hardcoded to `github.com` /
    /// `gitlab.com`, so treating `gitlab.example.org` as GitLab would silently
    /// route its API calls and OAuth to `gitlab.com` — the wrong instance. A
    /// self-hosted GitHub Enterprise host (`github.mycorp.com`) is already
    /// rejected this way (it matches neither `github.com` nor `.github.com`);
    /// GitLab is now symmetric — only exactly `gitlab.com` matches. Returning
    /// `None` lets callers reject the host rather than authenticate against the
    /// wrong one.
    pub fn from_host(host: &str) -> Option<Self> {
        let host = host.trim().to_ascii_lowercase();
        if host == "github.com" || host.ends_with(".github.com") {
            Some(Provider::GitHub)
        } else if host == "gitlab.com" {
            Some(Provider::GitLab)
        } else {
            None
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.key())
    }
}

/// A stored OAuth credential for a single provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    /// The provider this credential authenticates against.
    pub provider: Provider,
    /// The OAuth access token (bearer).
    pub access_token: String,
    /// The OAuth refresh token, when the provider issues one (GitLab does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Seconds-from-issue lifetime, when the provider reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    /// The OAuth token type (typically `bearer`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
}

impl Credential {
    /// Build a bare credential from a provider and access token.
    pub fn new(provider: Provider, access_token: impl Into<String>) -> Self {
        Self {
            provider,
            access_token: access_token.into(),
            refresh_token: None,
            expires_in: None,
            token_type: None,
        }
    }

    /// The value for an `Authorization` header.
    ///
    /// GitHub and GitLab both accept `Bearer <token>` on their v3/v4 APIs.
    pub fn authorization_header(&self) -> String {
        format!("Bearer {}", self.access_token)
    }

    /// Whether this credential carries a non-empty refresh token, i.e. it can be
    /// re-minted via [`refresh_access_token`](crate::oauth::refresh_access_token).
    /// GitLab issues one; GitHub OAuth App tokens are long-lived and do not.
    pub fn is_refreshable(&self) -> bool {
        self.refresh_token.as_deref().is_some_and(|t| !t.is_empty())
    }

    /// Whether this credential should be refreshed before use, given the unix
    /// timestamp it was obtained at and the current unix time.
    ///
    /// A [`REFRESH_SKEW_SECS`] margin re-mints the token just BEFORE it actually
    /// expires (so an in-flight request never races the expiry). The store
    /// persists `expires_in` but not the issue time, so the caller supplies
    /// `obtained_at_unix` (e.g. the credentials file's mtime, or a recorded
    /// issue time). Credentials the provider issued without an `expires_in`
    /// (GitHub OAuth tokens are long-lived) never expire here.
    pub fn needs_refresh(&self, obtained_at_unix: u64, now_unix: u64) -> bool {
        match self.expires_in {
            Some(ttl) => {
                let expires_at = obtained_at_unix.saturating_add(ttl);
                now_unix.saturating_add(REFRESH_SKEW_SECS) >= expires_at
            }
            None => false,
        }
    }
}

/// The clock-skew margin (seconds) applied by [`Credential::needs_refresh`] so a
/// token is re-minted just before it expires rather than exactly at expiry.
pub const REFRESH_SKEW_SECS: u64 = 300;

#[cfg(test)]
mod provider_tests {
    use super::*;

    #[test]
    fn provider_keys_hosts_and_display() {
        for (p, key, host, disp) in [
            (Provider::GitHub, "github", "github.com", "GitHub"),
            (Provider::GitLab, "gitlab", "gitlab.com", "GitLab"),
        ] {
            assert_eq!(p.key(), key);
            assert_eq!(p.default_host(), host);
            assert_eq!(p.display_name(), disp);
            assert_eq!(format!("{p}"), key);
        }
    }
}
