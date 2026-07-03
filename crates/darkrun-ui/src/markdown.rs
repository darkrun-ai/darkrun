//! A minimal, dependency-free markdown renderer for the short agent-authored
//! prose the interactive session views carry — question/direction prompts,
//! context preambles, and option descriptions.
//!
//! It covers exactly what those prompts use in practice: paragraphs (blank-line
//! separated), unordered lists (`- ` / `* `), inline `**bold**`, and inline
//! `` `code` ``. Everything else passes through as text. The output is a small
//! HTML string rendered via `dangerous_inner_html`; all source text is
//! HTML-escaped FIRST, then the inline markers are applied to the escaped text,
//! so there is no injection surface even though the input is agent-authored.
//!
//! This is deliberately not a full CommonMark engine (no headings, links,
//! nested lists, tables) — those don't appear in mid-run prompts, and keeping it
//! tiny avoids pulling a markdown crate into the wasm site build.

/// HTML-escape `&`, `<`, `>`, `"` so source text can never inject markup.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Apply inline markers to ALREADY-ESCAPED text: `**bold**` → `<strong>`,
/// `` `code` `` → `<code>`. Unmatched markers are left as literal text. Runs in
/// a single pass; `code` spans are taken verbatim (no bold inside code).
///
/// UTF-8 safe: the marker bytes (`*`, `` ` ``) are ASCII so `find` returns
/// char-boundary offsets, and untouched text is advanced one whole `char` at a
/// time — never byte-by-byte, which would shred multi-byte glyphs like `—`.
fn inline(escaped: &str) -> String {
    let mut out = String::with_capacity(escaped.len());
    let mut rest = escaped;
    while !rest.is_empty() {
        // Inline code: `...`
        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                out.push_str("<code class=\"dr-md-code\">");
                out.push_str(&after[..end]);
                out.push_str("</code>");
                rest = &after[end + 1..];
                continue;
            }
        }
        // Bold: **...**
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                out.push_str("<strong>");
                out.push_str(&after[..end]);
                out.push_str("</strong>");
                rest = &after[end + 2..];
                continue;
            }
        }
        // Pass one whole char through (advance by its full UTF-8 width).
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// Whether a line is an unordered-list item (`- ` or `* `), returning its body.
fn list_item(line: &str) -> Option<&str> {
    let t = line.trim_start();
    t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))
}

/// Whether a line is an ATX heading (`# ` … `###### `), returning its level
/// (1..=6) and the trimmed heading text. Up to three leading spaces are allowed
/// (CommonMark); more, or no space after the hashes, is not a heading.
fn heading(line: &str) -> Option<(u8, &str)> {
    let t = line.trim_start();
    let hashes = t.len() - t.trim_start_matches('#').len();
    if (1..=6).contains(&hashes) {
        if let Some(text) = t[hashes..].strip_prefix(' ') {
            return Some((hashes as u8, text.trim()));
        }
    }
    None
}

/// Whether a line opens/closes a fenced code block (``` optionally with a lang).
fn is_fence(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

/// Render the supported markdown subset of `src` to an HTML string. Empty input
/// yields an empty string. Blocks are paragraphs and `<ul>` lists; consecutive
/// list lines group into one list, blank lines break paragraphs.
pub fn to_html(src: &str) -> String {
    let mut out = String::new();
    let mut para: Vec<String> = Vec::new();
    let mut list: Vec<String> = Vec::new();

    let flush_para = |out: &mut String, para: &mut Vec<String>| {
        if !para.is_empty() {
            out.push_str("<p class=\"dr-md-p\">");
            out.push_str(&inline(&para.join(" ")));
            out.push_str("</p>");
            para.clear();
        }
    };
    let flush_list = |out: &mut String, list: &mut Vec<String>| {
        if !list.is_empty() {
            out.push_str("<ul class=\"dr-md-ul\">");
            for item in list.iter() {
                out.push_str("<li>");
                out.push_str(&inline(item));
                out.push_str("</li>");
            }
            out.push_str("</ul>");
            list.clear();
        }
    };

    // Fenced code blocks (```) are taken verbatim, so their state spans lines.
    let mut in_code = false;
    let mut code: Vec<String> = Vec::new();
    let flush_code = |out: &mut String, code: &mut Vec<String>| {
        out.push_str("<pre class=\"dr-md-pre\"><code>");
        out.push_str(&escape(&code.join("\n")));
        out.push_str("</code></pre>");
        code.clear();
    };

    for raw in src.lines() {
        // A fence line toggles code mode (and never appears in the output).
        if is_fence(raw) {
            if in_code {
                flush_code(&mut out, &mut code);
                in_code = false;
            } else {
                flush_list(&mut out, &mut list);
                flush_para(&mut out, &mut para);
                in_code = true;
            }
            continue;
        }
        if in_code {
            // Verbatim — the source line as-is (indentation is significant).
            code.push(raw.to_string());
            continue;
        }
        let line = raw.trim_end();
        if line.trim().is_empty() {
            flush_list(&mut out, &mut list);
            flush_para(&mut out, &mut para);
        } else if let Some((level, text)) = heading(line) {
            // A heading is its own block: close any open list/paragraph first.
            flush_list(&mut out, &mut list);
            flush_para(&mut out, &mut para);
            out.push_str(&format!("<h{level} class=\"dr-md-h{level}\">"));
            out.push_str(&inline(&escape(text)));
            out.push_str(&format!("</h{level}>"));
        } else if let Some(item) = list_item(line) {
            // A list starts: close any open paragraph first.
            flush_para(&mut out, &mut para);
            list.push(escape(item.trim()));
        } else {
            // A normal line: close any open list first.
            flush_list(&mut out, &mut list);
            para.push(escape(line.trim()));
        }
    }
    // Close anything still open — an unterminated fence flushes as a code block.
    if in_code && !code.is_empty() {
        flush_code(&mut out, &mut code);
    }
    flush_list(&mut out, &mut list);
    flush_para(&mut out, &mut para);
    out
}

/// Scoped CSS for markdown rendered by [`to_html`] — headings, paragraphs, lists,
/// inline code, and fenced code blocks under a `.dr-md` container. Inject this
/// once on any surface that renders `to_html` output (the session views ship
/// their own copy of the base rules; this is the self-contained set, including
/// headings + code fences, for other surfaces like the review artifact stage).
pub const CSS: &str = "\
.dr-md{font-family:var(--dr-font-sans);color:var(--dr-text);line-height:1.55;}\
.dr-md .dr-md-h1{font-size:20px;font-weight:700;margin:2px 0 10px;line-height:1.25;}\
.dr-md .dr-md-h2{font-size:16px;font-weight:700;margin:18px 0 8px;line-height:1.3;}\
.dr-md .dr-md-h3{font-size:14px;font-weight:700;margin:14px 0 6px;}\
.dr-md .dr-md-h4,.dr-md .dr-md-h5,.dr-md .dr-md-h6{font-size:13px;font-weight:700;margin:12px 0 4px;color:var(--dr-text-muted);}\
.dr-md .dr-md-h1:first-child,.dr-md .dr-md-h2:first-child{margin-top:0;}\
.dr-md .dr-md-p{margin:0 0 10px;}\
.dr-md .dr-md-p:last-child{margin-bottom:0;}\
.dr-md .dr-md-ul{margin:8px 0;padding-left:20px;display:flex;flex-direction:column;gap:5px;}\
.dr-md .dr-md-ul li{line-height:1.5;}\
.dr-md .dr-md-code{font-family:var(--dr-font-mono);font-size:0.92em;\
background:var(--dr-surface-overlay);border:1px solid var(--dr-border);\
border-radius:4px;padding:1px 5px;}\
.dr-md .dr-md-pre{font-family:var(--dr-font-mono);font-size:12.5px;line-height:1.5;\
background:var(--dr-surface-overlay);border:1px solid var(--dr-border);\
border-radius:8px;padding:12px 14px;overflow-x:auto;margin:10px 0;}\
.dr-md .dr-md-pre code{font-family:inherit;background:none;border:none;padding:0;white-space:pre;}\
.dr-md strong{font-weight:700;}\
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html() {
        assert_eq!(escape("a<b>&\"c"), "a&lt;b&gt;&amp;&quot;c");
    }

    #[test]
    fn renders_bold_and_code() {
        assert_eq!(
            to_html("call **run_tick** via `cargo test`"),
            "<p class=\"dr-md-p\">call <strong>run_tick</strong> via \
             <code class=\"dr-md-code\">cargo test</code></p>"
        );
    }

    #[test]
    fn renders_a_bulleted_list_distinct_from_paragraphs() {
        let html = to_html("Pick one:\n\n- **A** fast\n- B slow\n\ndone");
        assert!(html.contains("<p class=\"dr-md-p\">Pick one:</p>"), "{html}");
        assert!(
            html.contains("<ul class=\"dr-md-ul\"><li><strong>A</strong> fast</li><li>B slow</li></ul>"),
            "{html}"
        );
        assert!(html.ends_with("<p class=\"dr-md-p\">done</p>"), "{html}");
    }

    #[test]
    fn a_dash_run_on_splits_into_list_items() {
        // The real-world failure: bullets on their own lines must each become an
        // <li>, not one run-on paragraph with literal dashes.
        let html = to_html("- one\n- two\n- three");
        assert_eq!(html.matches("<li>").count(), 3, "{html}");
        assert!(!html.contains("- one"), "no literal dashes survive: {html}");
    }

    #[test]
    fn unmatched_markers_stay_literal_and_safe() {
        let html = to_html("2 * 3 and a lone ` tick");
        assert!(html.contains("2 * 3"), "{html}");
        assert!(!html.contains("<strong>"), "{html}");
        assert!(!html.contains("<code"), "{html}");
    }

    #[test]
    fn renders_atx_headings() {
        let html = to_html("# Unit: author-frame\n\n## Goal\n\nbody text");
        assert!(html.contains("<h1 class=\"dr-md-h1\">Unit: author-frame</h1>"), "{html}");
        assert!(html.contains("<h2 class=\"dr-md-h2\">Goal</h2>"), "{html}");
        assert!(html.contains("<p class=\"dr-md-p\">body text</p>"), "{html}");
        // A hash without a following space is NOT a heading.
        assert!(to_html("#nospace").contains("<p class=\"dr-md-p\">#nospace</p>"));
    }

    #[test]
    fn renders_fenced_code_verbatim_and_escaped() {
        let html = to_html("before\n\n```rust\nlet x = 1 < 2;\n```\n\nafter");
        assert!(
            html.contains("<pre class=\"dr-md-pre\"><code>let x = 1 &lt; 2;</code></pre>"),
            "{html}"
        );
        assert!(html.contains("<p class=\"dr-md-p\">before</p>"), "{html}");
        assert!(html.contains("<p class=\"dr-md-p\">after</p>"), "{html}");
        // The fence lines themselves never appear in the output.
        assert!(!html.contains("```"), "{html}");
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(to_html(""), "");
        assert_eq!(to_html("   \n  \n"), "");
    }

    #[test]
    fn multibyte_glyphs_survive_intact() {
        // Regression: an em-dash (3 UTF-8 bytes) next to bold/code must not be
        // shredded into mojibake by a byte-wise passthrough.
        let html = to_html("**A** — calls `run_tick` — fast … done");
        assert!(html.contains("</strong> — calls"), "{html}");
        assert!(html.contains("</code> — fast … done"), "{html}");
        assert!(!html.contains('\u{00e2}'), "no Latin-1 mojibake: {html}");
    }
}
