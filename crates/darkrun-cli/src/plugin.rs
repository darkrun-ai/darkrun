//! Plugin maintenance: generate per-harness command files from the canonical
//! Markdown commands.
//!
//! Claude Code reads slash commands as Markdown (`.md` with YAML frontmatter);
//! Gemini CLI and Kiro read them as TOML (`description = "..."` +
//! `prompt = """..."""`). The `.md` files in `plugin/commands/` are the single
//! source of truth — `darkrun plugin sync` regenerates the `.toml` siblings so
//! the two formats never drift (the same rot the thin-skill cleanup removed).

use std::fs;
use std::path::{Path, PathBuf};

/// Generate a `.toml` command beside every `.md` command under `dir`. Returns
/// the `.toml` paths written, sorted.
pub fn sync_commands(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let src = fs::read_to_string(&path)?;
        let (description, body) = split_command(&src);
        let toml = render_toml(&path, &description, &body);
        let toml_path = path.with_extension("toml");
        fs::write(&toml_path, toml)?;
        written.push(toml_path);
    }
    written.sort();
    Ok(written)
}

/// Split a command `.md` into `(description, body)`: pull `description:` out of
/// the YAML frontmatter and return everything after the closing `---` fence as
/// the prompt body. No frontmatter → empty description, whole text as body.
fn split_command(src: &str) -> (String, String) {
    let src = src.trim_start_matches('\u{feff}');
    if let Some(rest) = src.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let fm = &rest[..end];
            let body = rest[end + 4..].trim_start_matches('\n');
            let description = fm
                .lines()
                .find_map(|l| {
                    l.trim()
                        .strip_prefix("description:")
                        .map(|d| d.trim().trim_matches('"').to_string())
                })
                .unwrap_or_default();
            return (description, body.trim().to_string());
        }
    }
    (String::new(), src.trim().to_string())
}

/// Render a Gemini/Kiro-format TOML command.
fn render_toml(src: &Path, description: &str, body: &str) -> String {
    let name = src.file_stem().and_then(|s| s.to_str()).unwrap_or("command");
    // TOML basic-string escaping for the one-line description.
    let desc = description.replace('\\', "\\\\").replace('"', "\\\"");
    // The body is a TOML multi-line literal-ish basic string; guard the unlikely
    // `"""` collision so it can't terminate the string early.
    let body = body.replace("\"\"\"", "\"\"\\\"");
    format!(
        "# Generated from {name}.md by `darkrun plugin sync` — do not edit by hand.\n\
         description = \"{desc}\"\n\
         prompt = \"\"\"\n{body}\n\"\"\"\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_frontmatter_and_body() {
        let md = "---\ndescription: Do a thing\nargument-hint: [x]\n---\n\nThe body.\nLine two.\n";
        let (desc, body) = split_command(md);
        assert_eq!(desc, "Do a thing");
        assert_eq!(body, "The body.\nLine two.");
    }

    #[test]
    fn handles_missing_frontmatter() {
        let (desc, body) = split_command("Just a body, no frontmatter.");
        assert_eq!(desc, "");
        assert_eq!(body, "Just a body, no frontmatter.");
    }

    #[test]
    fn renders_toml_with_escaped_description() {
        let toml = render_toml(Path::new("darkrun-pickup.md"), "Say \"hi\"", "Body.");
        assert!(toml.contains("description = \"Say \\\"hi\\\"\""));
        assert!(toml.contains("prompt = \"\"\"\nBody.\n\"\"\""));
        assert!(toml.contains("Generated from darkrun-pickup.md"));
    }

    #[test]
    fn sync_writes_a_toml_per_md() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "---\ndescription: A\n---\nbody a").unwrap();
        fs::write(dir.path().join("b.md"), "---\ndescription: B\n---\nbody b").unwrap();
        // A stray non-md file is ignored.
        fs::write(dir.path().join("note.txt"), "ignore me").unwrap();
        let written = sync_commands(dir.path()).unwrap();
        assert_eq!(written.len(), 2);
        assert!(dir.path().join("a.toml").exists());
        assert!(dir.path().join("b.toml").exists());
        let a = fs::read_to_string(dir.path().join("a.toml")).unwrap();
        assert!(a.contains("description = \"A\""));
        assert!(a.contains("body a"));
    }
}
