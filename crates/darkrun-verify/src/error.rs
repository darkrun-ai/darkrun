//! The verification engine's error type.

use thiserror::Error;

/// An error raised while measuring objective evidence — driving the headless
/// browser, parsing the collected metrics, or running the load harness.
#[derive(Debug, Error)]
pub enum VerifyError {
    /// The headless browser could not be launched or driven.
    #[error("headless browser error: {0}")]
    Browser(String),

    /// A page evaluation returned a shape the analyzers could not read.
    #[error("could not read page metrics: {0}")]
    Metrics(String),

    /// The capture target (URL) was malformed.
    #[error("invalid target: {0}")]
    Target(String),

    /// The load harness could not reach or complete against its target.
    #[error("load harness error: {0}")]
    Load(String),

    /// Writing a captured artifact (screenshot, proof JSON) failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// (De)serializing a metrics payload or proof failed.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// A verification result.
pub type Result<T> = std::result::Result<T, VerifyError>;
