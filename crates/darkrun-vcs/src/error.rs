//! Error type for darkrun-vcs.

/// Errors produced by OAuth flows, credential storage, and provider REST calls.
#[derive(Debug, thiserror::Error)]
pub enum VcsError {
    /// The underlying HTTP transport failed before a response was produced.
    #[error("http transport error: {0}")]
    Transport(String),

    /// The provider returned a non-success HTTP status.
    #[error("{provider} api returned {status}: {message}")]
    Api {
        /// The provider that produced the error.
        provider: &'static str,
        /// The HTTP status code.
        status: u16,
        /// A human-readable message extracted from the error body.
        message: String,
    },

    /// The OAuth token endpoint returned an error payload.
    #[error("oauth token exchange failed: {error}{}", .description.as_deref().map(|d| format!(" ({d})")).unwrap_or_default())]
    OauthExchange {
        /// The machine-readable `error` code.
        error: String,
        /// The optional `error_description`.
        description: Option<String>,
    },

    /// A git remote URL could not be parsed into repo coordinates.
    #[error("could not parse repo coordinates from remote url: {0}")]
    RemoteParse(String),

    /// The credential store could not locate a usable home directory.
    #[error("could not determine credentials path: {0}")]
    CredentialsPath(String),

    /// A response body could not be decoded as expected.
    #[error("decode error: {0}")]
    Decode(String),

    /// A required field was missing from a provider response.
    #[error("missing field `{0}` in provider response")]
    MissingField(&'static str),

    /// JSON (de)serialization failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// An I/O operation against the credential store failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience alias for results in this crate.
pub type Result<T> = std::result::Result<T, VcsError>;
