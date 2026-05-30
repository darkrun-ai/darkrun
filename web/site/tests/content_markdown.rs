//! Integration tests for `render_markdown` and `Doc::to_html` — the
//! pulldown-cmark rendering used for docs, concepts, and blog prose.

use darkrun_site::content::{render_markdown, CONCEPTS, DOCS, POSTS};

#[test]
fn renders_a_heading() {
    let html = render_markdown("# Title");
    assert!(html.contains("<h1>"));
    assert!(html.contains("Title"));
    assert!(html.contains("</h1>"));
}

#[test]
fn renders_nested_heading_levels() {
    let html = render_markdown("## Two\n\n### Three");
    assert!(html.contains("<h2>"));
    assert!(html.contains("<h3>"));
}

#[test]
fn renders_bold_and_italic() {
    let html = render_markdown("**bold** and *italic*");
    assert!(html.contains("<strong>bold</strong>"));
    assert!(html.contains("<em>italic</em>"));
}

#[test]
fn renders_inline_code() {
    let html = render_markdown("use `darkrun` now");
    assert!(html.contains("<code>darkrun</code>"));
}

#[test]
fn renders_fenced_code_block() {
    let html = render_markdown("```\ncargo run\n```");
    assert!(html.contains("<pre>"));
    assert!(html.contains("<code>"));
    assert!(html.contains("cargo run"));
}

#[test]
fn renders_unordered_list() {
    let html = render_markdown("- one\n- two");
    assert!(html.contains("<ul>"));
    assert_eq!(html.matches("<li>").count(), 2);
}

#[test]
fn renders_ordered_list() {
    let html = render_markdown("1. first\n2. second");
    assert!(html.contains("<ol>"));
    assert_eq!(html.matches("<li>").count(), 2);
}

#[test]
fn renders_links() {
    let html = render_markdown("[darkrun](https://darkrun.ai)");
    assert!(html.contains("<a href=\"https://darkrun.ai\">"));
    assert!(html.contains("darkrun</a>"));
}

#[test]
fn renders_blockquote() {
    let html = render_markdown("> a quote");
    assert!(html.contains("<blockquote>"));
}

#[test]
fn tables_extension_is_enabled() {
    let html = render_markdown("| a | b |\n|---|---|\n| 1 | 2 |");
    assert!(html.contains("<table>"), "tables not rendered: {html}");
    assert!(html.contains("<th>"));
    assert!(html.contains("<td>"));
}

#[test]
fn strikethrough_extension_is_enabled() {
    let html = render_markdown("~~gone~~");
    assert!(html.contains("<del>gone</del>"), "strikethrough not rendered: {html}");
}

#[test]
fn footnotes_extension_is_enabled() {
    let html = render_markdown("text[^1]\n\n[^1]: the note");
    // pulldown-cmark emits footnote reference anchors when the extension is on.
    assert!(html.contains("footnote"), "footnotes not rendered: {html}");
}

#[test]
fn empty_input_yields_empty_output() {
    assert_eq!(render_markdown(""), "");
}

#[test]
fn plain_paragraph_is_wrapped() {
    let html = render_markdown("just words");
    assert!(html.contains("<p>just words</p>"));
}

#[test]
fn html_special_chars_in_text_are_escaped() {
    let html = render_markdown("a < b & c > d");
    assert!(html.contains("&lt;"));
    assert!(html.contains("&amp;"));
    assert!(html.contains("&gt;"));
}

#[test]
fn rendering_is_deterministic() {
    let src = "# H\n\nbody **x**";
    assert_eq!(render_markdown(src), render_markdown(src));
}

#[test]
fn doc_to_html_matches_render_markdown_of_its_body() {
    for d in DOCS.iter().chain(CONCEPTS).chain(POSTS) {
        assert_eq!(d.to_html(), render_markdown(d.markdown), "mismatch on {}", d.slug);
    }
}

#[test]
fn every_doc_renders_a_leading_h1() {
    for d in DOCS.iter().chain(CONCEPTS).chain(POSTS) {
        let html = d.to_html();
        assert!(html.contains("<h1>"), "{} has no <h1>", d.slug);
    }
}

#[test]
fn every_doc_renders_non_empty_html() {
    for d in DOCS.iter().chain(CONCEPTS).chain(POSTS) {
        assert!(!d.to_html().trim().is_empty(), "{} rendered empty", d.slug);
    }
}

#[test]
fn getting_started_renders_its_install_section() {
    let d = DOCS.iter().find(|d| d.slug == "getting-started").unwrap();
    let html = d.to_html();
    assert!(html.contains("Install"));
    // getting-started.md uses a fenced code block in its install steps.
    assert!(html.contains("<pre>"));
}

#[test]
fn glossary_renders_a_definition_list_style_bullets() {
    let d = CONCEPTS.iter().find(|d| d.slug == "glossary").unwrap();
    let html = d.to_html();
    assert!(html.contains("<ul>"));
    assert!(html.contains("<strong>"));
}
