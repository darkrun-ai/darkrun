//! Error type for the content loader and validator.

use darkrun_core::CoreError;

/// Errors produced while loading or validating embedded factory content.
#[derive(Debug, thiserror::Error)]
pub enum ContentError {
    /// A frontmatter/markdown document failed to parse.
    #[error("failed to parse content: {0}")]
    Core(#[from] CoreError),

    /// An expected embedded file was missing from the corpus.
    #[error("embedded content file not found: {0}")]
    FileNotFound(String),

    /// A requested factory does not exist in the corpus.
    #[error("factory not found: {0}")]
    FactoryNotFound(String),

    /// A factory's content failed a structural validation rule.
    #[error("invalid factory `{factory}`: {message}")]
    Invalid {
        /// The factory slug that failed validation.
        factory: String,
        /// What was wrong.
        message: String,
    },
}

/// Convenience alias for results in this crate.
pub type Result<T> = std::result::Result<T, ContentError>;
