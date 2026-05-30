//! stdio MCP server entry point.
//!
//! [`serve_stdio`] is the function `darkrun-cli` calls to run the manager
//! as an MCP server over stdin/stdout. It serves the [`DarkrunServer`] tool
//! surface until the client disconnects.

use std::path::PathBuf;

use rmcp::transport::io::stdio;
use rmcp::ServiceExt;

use crate::tools::DarkrunServer;

/// Serve the darkrun MCP server over stdio, rooted at `repo_root`.
///
/// Blocks until the MCP client disconnects. State lives under
/// `<repo_root>/.darkrun`.
pub async fn serve_stdio(repo_root: impl Into<PathBuf>) -> std::io::Result<()> {
    let server = DarkrunServer::new(repo_root);
    let running = server
        .serve(stdio())
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    running
        .waiting()
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}
