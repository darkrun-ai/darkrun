//! Generator binary — writes the OpenAPI document to `openapi.json` at the
//! crate root.
//!
//! Run with `cargo run -p darkrun-api --bin emit_openapi`. The emitted file is
//! checked in and guarded by the `openapi_json_is_in_sync` parity test, so any
//! drift between the wire types and the committed contract fails CI.

use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let mut text = darkrun_api::openapi::document_json();
    text.push('\n');

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("openapi.json");
    std::fs::write(&path, text)?;
    println!("wrote {}", path.display());
    Ok(())
}
