//! The shared markdown renderer for agent-authored prose, specs, and review
//! artifacts — unit specs, run overviews, question/direction prompts, output
//! documents.
//!
//! It renders full CommonMark + GFM (headings, paragraphs, ordered/nested lists,
//! blockquotes, tables, links, images, inline code, fenced code, strikethrough)
//! via `pulldown-cmark` — the same engine the marketing site uses, so the two
//! surfaces stay in lockstep. The output is an HTML string dropped into a
//! `.dr-md` container via `dangerous_inner_html`.
//!
//! ## Safety (the input is agent-authored)
//!
//! `pulldown-cmark` renders text content HTML-escaped, but by default it passes
//! RAW HTML through verbatim (`<script>…`), which would be an injection surface
//! for agent-authored input. [`to_html`] closes that: every raw HTML event
//! (block and inline) is rewritten to escaped text, so markup in the source can
//! never inject nodes. Link/image destinations are also scheme-checked, so a
//! `javascript:` URL degrades to an inert `#`.
//!
//! ## Frontmatter
//!
//! A leading YAML `---` block is NOT part of the rendered body: [`to_html`]
//! strips it, and [`split_frontmatter`] / [`frontmatter_html`] surface it as a
//! clean metadata header (status/station/role/mode chips) above the prose,
//! rather than leaking `---` + literal `key: value` lines into the document.

use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag};

/// HTML-escape `&`, `<`, `>`, `"` so source text can never inject markup. Used
/// for the frontmatter chips (the body is escaped by `pulldown-cmark` itself).
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

/// Whether a link/image destination uses a safe scheme. Relative URLs (no
/// scheme) and the `http`/`https`/`mailto`/`tel` schemes are allowed; anything
/// else (`javascript:`, `data:`, `vbscript:`, `file:`) is rejected.
fn is_safe_url(url: &str) -> bool {
    let u = url.trim();
    // Find the scheme delimiter, but only if it precedes any path/query/fragment
    // separator — otherwise a colon later in a relative path isn't a scheme.
    let mut scheme_end = None;
    for (i, c) in u.char_indices() {
        match c {
            ':' => {
                scheme_end = Some(i);
                break;
            }
            '/' | '?' | '#' => break,
            c if c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.' => {}
            _ => break,
        }
    }
    match scheme_end {
        Some(i) => matches!(
            u[..i].to_ascii_lowercase().as_str(),
            "http" | "https" | "mailto" | "tel"
        ),
        None => true,
    }
}

/// Replace an unsafe destination with an inert `#`.
fn sanitize_url(url: CowStr<'_>) -> CowStr<'_> {
    if is_safe_url(&url) {
        url
    } else {
        CowStr::Borrowed("#")
    }
}

/// Render markdown `src` to an HTML string for a `.dr-md` container. Full
/// CommonMark + GFM (tables, strikethrough); frontmatter is stripped; raw HTML
/// is escaped and unsafe link/image URLs are neutralized (agent-safe). Empty
/// input yields an empty string.
pub fn to_html(src: &str) -> String {
    let (_, body) = split_frontmatter(src);
    if body.trim().is_empty() {
        return String::new();
    }
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(body, options).map(|event| match event {
        // Agent-authored input: never pass raw HTML through — escaping it to
        // text removes the injection surface while keeping it visible.
        Event::Html(h) => Event::Text(h),
        Event::InlineHtml(h) => Event::Text(h),
        // Scheme-check link/image destinations so `javascript:` can't ride in.
        Event::Start(Tag::Link { link_type, dest_url, title, id }) => Event::Start(Tag::Link {
            link_type,
            dest_url: sanitize_url(dest_url),
            title,
            id,
        }),
        Event::Start(Tag::Image { link_type, dest_url, title, id }) => Event::Start(Tag::Image {
            link_type,
            dest_url: sanitize_url(dest_url),
            title,
            id,
        }),
        other => other,
    });
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

/// Split a leading YAML frontmatter block off `src`, returning `(frontmatter,
/// body)`. A frontmatter block is a first line of exactly `---` closed by a
/// later line of exactly `---` or `...`. With no such block the whole input is
/// the body and the frontmatter is `None`.
pub fn split_frontmatter(src: &str) -> (Option<&str>, &str) {
    let s = src.strip_prefix('\u{feff}').unwrap_or(src);
    let first_end = s.find('\n').unwrap_or(s.len());
    if s[..first_end].trim_end() != "---" {
        return (None, src);
    }
    let after_open = &s[(first_end + 1).min(s.len())..];
    let mut idx = 0usize;
    for line in after_open.split_inclusive('\n') {
        let trimmed = line.trim_matches(|c| c == '\n' || c == '\r' || c == ' ' || c == '\t');
        if trimmed == "---" || trimmed == "..." {
            let fm = after_open[..idx].trim_matches('\n');
            let body = &after_open[(idx + line.len()).min(after_open.len())..];
            return (Some(fm), body);
        }
        idx += line.len();
    }
    (None, src)
}

/// Parse a frontmatter block into ordered `key: value` pairs. A flat YAML
/// subset: `key: value` lines (quotes trimmed), skipping blanks, comments, and
/// non-`key: value` lines (nested structures aren't modelled).
pub fn frontmatter_pairs(block: &str) -> Vec<(String, String)> {
    block
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
                return None;
            }
            let (k, v) = line.split_once(':')?;
            let k = k.trim();
            if k.is_empty() {
                return None;
            }
            let v = v.trim().trim_matches('"').trim_matches('\'').trim();
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// A tone-class suffix for a `status:` value so a completed unit reads green, a
/// blocked one red, and in-flight amber/cyan.
fn status_tone_class(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "done" | "complete" | "completed" | "passed" | "approved" | "merged" | "ok" => {
            " dr-md-chip-ok"
        }
        "blocked" | "failed" | "error" | "rejected" | "changes_requested" => " dr-md-chip-danger",
        "active" | "in_progress" | "in-progress" | "running" | "review" | "pending" => {
            " dr-md-chip-active"
        }
        _ => "",
    }
}

/// Render a frontmatter block as a clean metadata header — a row of key/value
/// chips (status toned by state) — for placement above the rendered body inside
/// a `.dr-md` container. Empty (or key-less) input yields an empty string, so a
/// caller can unconditionally prepend it.
pub fn frontmatter_html(block: &str) -> String {
    let pairs = frontmatter_pairs(block);
    if pairs.is_empty() {
        return String::new();
    }
    let mut out = String::from("<div class=\"dr-md-meta\">");
    for (k, v) in &pairs {
        if v.is_empty() {
            out.push_str(&format!(
                "<span class=\"dr-md-chip\"><span class=\"dr-md-chip-k\">{}</span></span>",
                escape(k),
            ));
            continue;
        }
        let tone = if k.eq_ignore_ascii_case("status") {
            status_tone_class(v)
        } else {
            ""
        };
        out.push_str(&format!(
            "<span class=\"dr-md-chip{tone}\">\
             <span class=\"dr-md-chip-k\">{}</span>\
             <span class=\"dr-md-chip-v\">{}</span></span>",
            escape(k),
            escape(v),
        ));
    }
    out.push_str("</div>");
    out
}

/// A content heuristic: whether `src` reads as markdown (structural or inline
/// signals) rather than plain source. Used to decide whether a text artifact
/// with no telling extension should render formatted or raw. Conservative: it
/// keys on real markdown syntax, so code/prose without markers stays raw.
pub fn looks_like_markdown(src: &str) -> bool {
    // A frontmatter block is a strong signal on its own.
    if split_frontmatter(src).0.is_some() {
        return true;
    }
    let body = split_frontmatter(src).1;
    for raw in body.lines() {
        let line = raw.trim_start();
        // ATX heading (`# ` … `###### `).
        let hashes = line.len() - line.trim_start_matches('#').len();
        if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
            return true;
        }
        // Unordered list.
        if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
            return true;
        }
        // Blockquote.
        if line == ">" || line.starts_with("> ") {
            return true;
        }
        // Fenced code.
        if line.starts_with("```") || line.starts_with("~~~") {
            return true;
        }
        // Table row.
        if line.starts_with('|') && line.matches('|').count() >= 2 {
            return true;
        }
        // Ordered list (`1. `).
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && line[digits..].starts_with(". ") {
            return true;
        }
    }
    // Inline signals: bold, strikethrough, a matched code span, or a link/image.
    body.contains("**") || body.contains("~~") || body.matches('`').count() >= 2 || body.contains("](")
}

/// Scoped CSS for markdown rendered by [`to_html`], plus the frontmatter chip
/// header. The single source of truth for `.dr-md` styling — inject it once on
/// any surface that renders `to_html` output (session views, the review
/// annotate stage, the artifact browser) so headings, code, tables, and
/// blockquotes look identical everywhere.

/// Render a whole markdown DOCUMENT: a doc with frontmatter gets its pairs as a
/// chip row ([`frontmatter_html`]) ahead of the rendered body instead of a raw
/// `---` block; a doc without frontmatter renders exactly like [`to_html`].
/// This is the entry the artifact-browsing surfaces (annotate stage, knowledge
/// tab, reflection) share.
///
/// A fenced leading block with NO flat scalars (a YAML list, nested structure)
/// cannot be summarized as chips; rather than silently dropping it, the WHOLE
/// document renders as body so no content ever vanishes.
pub fn to_html_doc(src: &str) -> String {
    match split_frontmatter(src) {
        (Some(block), body) => {
            let header = frontmatter_html(block);
            if header.is_empty() {
                // The leading `---` block held no flat scalars, so it is NOT a
                // metadata header. It must not vanish: `to_html` drops a leading
                // `---` fence as YAML metadata, so rendering the raw source would
                // eat the fenced lines. Reconstruct without the fence markers so
                // the content (a list, prose) renders as ordinary markdown.
                if block.trim().is_empty() {
                    return to_html(body);
                }
                return to_html(&format!("{block}\n\n{body}"));
            }
            format!("{header}{}", to_html(body))
        }
        (None, body) => to_html(body),
    }
}

pub const CSS: &str = "\
.dr-md{font-family:var(--dr-font-sans);color:var(--dr-text);line-height:1.55;}\
.dr-md h1{font-size:20px;font-weight:700;margin:2px 0 10px;line-height:1.25;}\
.dr-md h2{font-size:16px;font-weight:700;margin:18px 0 8px;line-height:1.3;}\
.dr-md h3{font-size:14px;font-weight:700;margin:14px 0 6px;}\
.dr-md h4,.dr-md h5,.dr-md h6{font-size:13px;font-weight:700;margin:12px 0 4px;color:var(--dr-text-muted);}\
.dr-md h1:first-child,.dr-md h2:first-child,.dr-md h3:first-child{margin-top:0;}\
.dr-md p{margin:0 0 10px;}\
.dr-md>:last-child{margin-bottom:0;}\
.dr-md ul,.dr-md ol{margin:8px 0;padding-left:22px;display:flex;flex-direction:column;gap:5px;}\
.dr-md li{line-height:1.5;}\
.dr-md li>ul,.dr-md li>ol{margin:5px 0 0;}\
.dr-md li::marker{color:var(--dr-text-faint);}\
.dr-md a{color:var(--dr-accent);text-decoration:none;\
border-bottom:1px solid color-mix(in srgb,var(--dr-accent) 40%,transparent);}\
.dr-md a:hover{border-bottom-color:var(--dr-accent);}\
.dr-md strong{font-weight:700;color:var(--dr-text);}\
.dr-md em{font-style:italic;}\
.dr-md del{opacity:0.6;}\
.dr-md code{font-family:var(--dr-font-mono);font-size:0.92em;\
background:var(--dr-surface-overlay);border:1px solid var(--dr-border);\
border-radius:4px;padding:1px 5px;}\
.dr-md pre{font-family:var(--dr-font-mono);font-size:12.5px;line-height:1.5;\
background:var(--dr-surface-overlay);border:1px solid var(--dr-border);\
border-radius:8px;padding:12px 14px;overflow-x:auto;margin:10px 0;}\
.dr-md pre code{font-family:inherit;font-size:inherit;background:none;border:none;padding:0;white-space:pre;}\
.dr-md blockquote{margin:10px 0;padding:2px 14px;\
border-left:3px solid var(--dr-border-strong);color:var(--dr-text-muted);}\
.dr-md blockquote p{margin:6px 0;}\
.dr-md img{max-width:100%;border-radius:6px;border:1px solid var(--dr-border);}\
.dr-md hr{border:none;border-top:1px solid var(--dr-border);margin:14px 0;}\
.dr-md table{border-collapse:collapse;margin:10px 0;font-size:12.5px;\
max-width:100%;display:block;overflow-x:auto;}\
.dr-md th,.dr-md td{border:1px solid var(--dr-border);padding:5px 10px;text-align:left;}\
.dr-md thead th{background:var(--dr-surface-overlay);font-weight:700;}\
.dr-md-meta{display:flex;flex-wrap:wrap;gap:6px;margin:0 0 14px;}\
.dr-md-chip{display:inline-flex;align-items:center;gap:6px;\
font-family:var(--dr-font-mono);font-size:11px;border:1px solid var(--dr-border);\
border-radius:6px;padding:2px 4px 2px 8px;background:var(--dr-surface-raised);}\
.dr-md-chip-k{color:var(--dr-text-faint);text-transform:uppercase;letter-spacing:0.05em;}\
.dr-md-chip-v{color:var(--dr-text);background:var(--dr-surface-overlay);border-radius:4px;padding:1px 7px;}\
.dr-md-chip-ok .dr-md-chip-v{color:var(--dr-status-ok);}\
.dr-md-chip-danger .dr-md-chip-v{color:var(--dr-status-danger);}\
.dr-md-chip-active .dr-md-chip-v{color:var(--dr-status-info);}\
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html() {
        assert_eq!(escape("a<b>&\"c"), "a&lt;b&gt;&amp;&quot;c");
    }

    #[test]
    fn renders_paragraph_bold_and_inline_code() {
        let html = to_html("call **run_tick** via `cargo test`");
        assert!(html.contains("<p>call <strong>run_tick</strong> via <code>cargo test</code></p>"), "{html}");
    }

    #[test]
    fn renders_gfm_table() {
        let html = to_html("| A | B |\n|---|---|\n| 1 | 2 |");
        assert!(html.contains("<table>"), "{html}");
        assert!(html.contains("<th>A</th>"), "{html}");
        assert!(html.contains("<td>1</td>"), "{html}");
    }

    #[test]
    fn renders_links() {
        let html = to_html("see [the docs](https://example.com/docs)");
        assert!(html.contains("<a href=\"https://example.com/docs\">the docs</a>"), "{html}");
    }

    #[test]
    fn renders_ordered_and_nested_lists() {
        let html = to_html("1. first\n2. second\n   - nested a\n   - nested b");
        assert!(html.contains("<ol>"), "ordered list: {html}");
        assert!(html.contains("<li>first"), "{html}");
        // The nested bullets live inside the second item's own <ul>.
        assert!(html.contains("<ul>"), "nested unordered list: {html}");
        assert!(html.contains("<li>nested a</li>"), "{html}");
    }

    #[test]
    fn renders_blockquote() {
        let html = to_html("> a quoted line\n> continued");
        assert!(html.contains("<blockquote>"), "{html}");
        assert!(html.contains("a quoted line"), "{html}");
    }

    #[test]
    fn renders_strikethrough() {
        let html = to_html("this is ~~gone~~ now");
        assert!(html.contains("<del>gone</del>"), "{html}");
    }

    #[test]
    fn renders_fenced_code_escaped() {
        let html = to_html("```rust\nlet x = 1 < 2;\n```");
        assert!(html.contains("<pre>"), "{html}");
        assert!(html.contains("let x = 1 &lt; 2;"), "{html}");
    }

    #[test]
    fn raw_block_html_is_escaped_not_injected() {
        let html = to_html("<script>alert(1)</script>");
        assert!(!html.contains("<script>"), "raw script must not pass through: {html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
    }

    #[test]
    fn raw_inline_html_is_escaped_not_injected() {
        let html = to_html("a normal <b>bold?</b> line");
        assert!(!html.contains("<b>"), "inline raw html must not pass through: {html}");
        assert!(html.contains("&lt;b&gt;"), "{html}");
    }

    #[test]
    fn unsafe_link_scheme_is_neutralized() {
        let html = to_html("[x](javascript:alert(document.cookie))");
        assert!(!html.contains("javascript:"), "{html}");
        assert!(html.contains("href=\"#\""), "{html}");
    }

    #[test]
    fn safe_url_allows_relative_and_common_schemes() {
        assert!(is_safe_url("https://example.com"));
        assert!(is_safe_url("http://example.com"));
        assert!(is_safe_url("mailto:a@b.co"));
        assert!(is_safe_url("/relative/path"));
        assert!(is_safe_url("#anchor"));
        assert!(is_safe_url("./a:b")); // colon after a path separator isn't a scheme
        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url("data:text/html,<script>"));
        assert!(!is_safe_url("vbscript:msgbox"));
    }

    #[test]
    fn frontmatter_is_split_out_and_not_leaked_into_body() {
        let src = "---\nstatus: done\nstation: build\nrole: worker\n---\n\n# Spec\n\nthe body";
        let (fm, body) = split_frontmatter(src);
        assert_eq!(fm, Some("status: done\nstation: build\nrole: worker"));
        assert!(body.trim_start().starts_with("# Spec"), "{body}");
        let html = to_html(src);
        assert!(html.contains("<h1>Spec</h1>"), "{html}");
        assert!(!html.contains("status: done"), "frontmatter leaked into body: {html}");
        assert!(!html.contains("---"), "fence leaked into body: {html}");
    }

    #[test]
    fn no_frontmatter_leaves_body_intact() {
        let src = "# Just a heading\n\nno frontmatter here";
        let (fm, body) = split_frontmatter(src);
        assert_eq!(fm, None);
        assert_eq!(body, src);
    }

    #[test]
    fn frontmatter_pairs_parse_flat_yaml() {
        let pairs = frontmatter_pairs("title: \"My Run\"\nmode: quick\n# a comment\nbad line");
        assert_eq!(
            pairs,
            vec![
                ("title".to_string(), "My Run".to_string()),
                ("mode".to_string(), "quick".to_string()),
            ]
        );
    }

    #[test]
    fn frontmatter_html_renders_toned_chips() {
        let html = frontmatter_html("status: done\nstation: build");
        assert!(html.contains("dr-md-meta"), "{html}");
        assert!(html.contains("dr-md-chip-ok"), "status=done should tone ok: {html}");
        assert!(html.contains("station"), "{html}");
        assert!(html.contains("build"), "{html}");
        // A blocked status tones danger.
        assert!(frontmatter_html("status: blocked").contains("dr-md-chip-danger"));
        // No pairs → empty (a caller can prepend unconditionally).
        assert_eq!(frontmatter_html(""), "");
    }

    #[test]
    fn looks_like_markdown_detects_structure() {
        assert!(looks_like_markdown("# Heading"));
        assert!(looks_like_markdown("- a bullet"));
        assert!(looks_like_markdown("1. an item"));
        assert!(looks_like_markdown("> a quote"));
        assert!(looks_like_markdown("```\ncode\n```"));
        assert!(looks_like_markdown("| a | b |\n|---|---|"));
        assert!(looks_like_markdown("some **bold** prose"));
        assert!(looks_like_markdown("a [link](https://x.co)"));
        assert!(looks_like_markdown("---\nstatus: done\n---\nbody"));
    }

    #[test]
    fn looks_like_markdown_ignores_plain_and_codey_text() {
        assert!(!looks_like_markdown("just some plain prose with no markers at all"));
        assert!(!looks_like_markdown("fn main() { let x = 1; println(x); }"));
        assert!(!looks_like_markdown(""));
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(to_html(""), "");
        assert_eq!(to_html("   \n  \n"), "");
    }

    #[test]
    fn frontmatter_renders_as_chips_ahead_of_the_body() {
        let doc = "---\nid: FB-01\nstatus: pending\nreplies:\n  - \"skip me\"\n---\n# Title\n\nbody";
        let (fm, body) = split_frontmatter(doc);
        let pairs = frontmatter_pairs(fm.expect("frontmatter block"));
        // Flat scalars only; the list items under `replies:` are skipped.
        assert!(pairs.contains(&("id".to_string(), "FB-01".to_string())), "{pairs:?}");
        assert!(pairs.contains(&("status".to_string(), "pending".to_string())), "{pairs:?}");
        assert!(body.trim_start().starts_with("# Title"));
        let html = to_html_doc(doc);
        assert!(html.starts_with("<div class=\"dr-md-meta\">"), "{html}");
        assert!(html.contains("FB-01"), "{html}");
        assert!(html.contains("<h1>Title</h1>"), "{html}");
        // The fence lines never render as body text.
        assert!(!html.contains("<p>---"), "{html}");
    }

    #[test]
    fn fenced_block_with_no_flat_scalars_is_never_silently_dropped() {
        // The leading fenced section holds only a YAML list: unsummarizable as
        // chips, so the WHOLE doc renders as body rather than the section
        // vanishing from the render.
        let doc = "---\n- alpha\n- beta\n---\nbody text";
        let html = to_html_doc(doc);
        assert!(html.contains("alpha"), "fenced content must not vanish: {html}");
        assert!(html.contains("body text"), "{html}");
    }

    #[test]
    fn doc_without_frontmatter_renders_like_to_html() {
        assert_eq!(to_html_doc("plain **bold**"), to_html("plain **bold**"));
        // An unclosed fence is body, not frontmatter (nothing silently dropped).
        let unclosed = "---\ntitle: x\nno closing fence";
        assert_eq!(to_html_doc(unclosed), to_html(unclosed));
        // A thematic-break-looking doc with no leading fence stays untouched.
        assert!(to_html_doc("a\n\n---\n\nb").contains("a"));
    }

    #[test]
    fn frontmatter_values_are_escaped_in_chips() {
        let doc = "---\ntitle: \"<b>sneaky</b>\"\n---\nbody";
        let html = to_html_doc(doc);
        assert!(html.contains("&lt;b&gt;sneaky&lt;/b&gt;"), "{html}");
        assert!(!html.contains("<b>sneaky</b>"), "{html}");
    }

    #[test]
    fn multibyte_glyphs_survive_intact() {
        // An em-dash / ellipsis (multi-byte) next to bold/code must not shred.
        let html = to_html("**A** — calls `run_tick` — fast … done");
        assert!(html.contains("<strong>A</strong> — calls <code>run_tick</code> — fast … done"), "{html}");
        assert!(!html.contains('\u{00e2}'), "no Latin-1 mojibake: {html}");
    }
}

