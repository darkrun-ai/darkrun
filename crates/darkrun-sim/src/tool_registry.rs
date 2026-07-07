//! The canonical set of darkrun MCP tool names.
//!
//! The fidelity suite needs the *real* set of tools an agent will have, so it
//! can flag any tool a prompt names that does not exist. The obvious source is
//! `darkrun_mcp::tools::DarkrunServer::tool_router().list_all()`, but rmcp's
//! `#[tool_router]` macro generates that accessor **crate-private** (`vis:
//! None`), so it is unreachable from here.
//!
//! Instead we read the single source of truth those tools are declared in —
//! `crates/darkrun-mcp/src/tools.rs`, where every tool carries a
//! `#[tool(name = "darkrun_…")]` attribute — and extract the names. The file is
//! embedded at **compile time** via [`include_str!`], so this recompiles when
//! the tool surface changes and fails loudly (a build error) if darkrun-mcp is
//! ever restructured out from under it.

use std::collections::BTreeSet;

/// The darkrun-mcp tool-handler source, baked in at compile time. Every MCP tool
/// is declared here with a `#[tool(name = "…")]` attribute.
const MCP_TOOLS_SRC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../darkrun-mcp/src/tools.rs"
));

/// Every registered darkrun MCP tool name, parsed from the tool declarations.
pub fn known_tool_names() -> BTreeSet<String> {
    parse_tool_names(MCP_TOOLS_SRC)
}

/// Extract every `name = "darkrun_…"` value from Rust source. Matches the
/// `#[tool(name = "…")]` attribute form (with any spacing), and only accepts
/// values under the `darkrun_` namespace, so tool-name strings in prose or in
/// `description = "…"` fields are never mistaken for declarations.
fn parse_tool_names(src: &str) -> BTreeSet<String> {
    let bytes = src.as_bytes();
    let mut names = BTreeSet::new();
    let mut search = 0usize;
    while let Some(rel) = src[search..].find("name") {
        let at = search + rel;
        search = at + 4;
        // `name` must be a whole word — reject `filename`, `nickname`, etc.
        if at > 0 {
            let prev = bytes[at - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        let mut j = at + 4;
        j = skip_ws(bytes, j);
        if bytes.get(j) != Some(&b'=') {
            continue;
        }
        j = skip_ws(bytes, j + 1);
        if bytes.get(j) != Some(&b'"') {
            continue;
        }
        let val_start = j + 1;
        let mut k = val_start;
        while k < bytes.len() && bytes[k] != b'"' {
            k += 1;
        }
        let val = &src[val_start..k];
        if val.starts_with("darkrun_") {
            names.insert(val.to_string());
        }
    }
    names
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_real_tool_surface() {
        let tools = known_tool_names();
        // A healthy floor — the surface is dozens of tools, not a handful.
        assert!(
            tools.len() >= 40,
            "expected the full tool surface, parsed only {}: {tools:?}",
            tools.len()
        );
        // Anchors the fidelity suite leans on must all be present.
        for anchor in [
            "darkrun_advance",
            "darkrun_tick",
            "darkrun_unit_create",
            "darkrun_unit_iterate",
            "darkrun_checkpoint_decide",
            "darkrun_review_stamp",
            "darkrun_brief_record",
            "darkrun_reflection_record",
            "darkrun_question",
        ] {
            assert!(tools.contains(anchor), "missing anchor tool {anchor}");
        }
    }

    #[test]
    fn ignores_tool_names_in_prose() {
        // A `description` mentioning a tool must not be picked up as a
        // declaration — only `name = "…"` counts.
        let src = r#"
            #[tool(name = "darkrun_real", description = "Deprecated alias of darkrun_ghost.")]
            fn x() {}
            let filename = "darkrun_not_a_tool";
        "#;
        let names = parse_tool_names(src);
        assert!(names.contains("darkrun_real"));
        assert!(!names.contains("darkrun_ghost"), "prose mention leaked in");
        assert!(!names.contains("darkrun_not_a_tool"), "`filename` matched");
    }
}
