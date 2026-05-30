//! Integration tests for the RSS 2.0 feed builder (`feed.xml`).

use darkrun_site::content::POSTS;
use darkrun_site::seo::{self, SITE_DESCRIPTION, SITE_NAME, SITE_URL};

fn extract_all(xml: &str, open: &str, close: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(open) {
        let after = &rest[start + open.len()..];
        let end = after.find(close).expect("closing tag");
        out.push(after[..end].to_string());
        rest = &after[end + close.len()..];
    }
    out
}

#[test]
fn starts_with_xml_declaration() {
    assert!(seo::feed_rss().starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
}

#[test]
fn declares_rss_2_0() {
    assert!(seo::feed_rss().contains("<rss version=\"2.0\">"));
}

#[test]
fn has_a_single_channel() {
    let xml = seo::feed_rss();
    assert_eq!(xml.matches("<channel>").count(), 1);
    assert_eq!(xml.matches("</channel>").count(), 1);
}

#[test]
fn channel_carries_site_metadata() {
    let xml = seo::feed_rss();
    assert!(xml.contains(&format!("<title>{SITE_NAME}</title>")));
    assert!(xml.contains(&format!("<link>{SITE_URL}</link>")));
    assert!(xml.contains(&format!("<description>{SITE_DESCRIPTION}</description>")));
}

#[test]
fn item_count_matches_post_count() {
    assert_eq!(seo::feed_rss().matches("<item>").count(), POSTS.len());
}

#[test]
fn item_open_close_balanced() {
    let xml = seo::feed_rss();
    assert_eq!(xml.matches("<item>").count(), xml.matches("</item>").count());
}

#[test]
fn every_post_appears_with_title_and_link() {
    let xml = seo::feed_rss();
    for post in POSTS {
        assert!(xml.contains(&format!("<title>{}</title>", post.title)), "title {}", post.title);
        let link = format!("{SITE_URL}/blog/{}", post.slug);
        assert!(xml.contains(&format!("<link>{link}</link>")), "link {link}");
    }
}

#[test]
fn guid_equals_link_per_item() {
    let xml = seo::feed_rss();
    for post in POSTS {
        let link = format!("{SITE_URL}/blog/{}", post.slug);
        assert!(xml.contains(&format!("<guid>{link}</guid>")), "guid {link}");
    }
}

/// XML-escape the five entities, matching the builder's escaping.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[test]
fn every_post_summary_is_present_xml_escaped() {
    let xml = seo::feed_rss();
    for post in POSTS {
        // description holds the post summary (the channel description is the site one).
        assert!(xml.contains(&format!("<description>{}</description>", xml_escape(post.summary))));
    }
}

#[test]
fn apostrophes_in_summaries_are_escaped() {
    let xml = seo::feed_rss();
    if POSTS.iter().any(|p| p.summary.contains('\'')) {
        assert!(xml.contains("&apos;"));
    }
}

#[test]
fn item_links_point_under_blog() {
    for link in extract_all(&seo::feed_rss(), "<link>", "</link>") {
        if link == SITE_URL {
            continue; // the channel self-link
        }
        assert!(link.starts_with(&format!("{SITE_URL}/blog/")), "stray link {link}");
    }
}

#[test]
fn item_order_matches_posts_order() {
    // Item links, in document order, must match POSTS order (newest first).
    let links: Vec<String> = extract_all(&seo::feed_rss(), "<link>", "</link>")
        .into_iter()
        .filter(|l| l != SITE_URL)
        .collect();
    let expected: Vec<String> =
        POSTS.iter().map(|p| format!("{SITE_URL}/blog/{}", p.slug)).collect();
    assert_eq!(links, expected);
}

#[test]
fn is_deterministic() {
    assert_eq!(seo::feed_rss(), seo::feed_rss());
}

#[test]
fn ends_with_closing_rss() {
    assert!(seo::feed_rss().trim_end().ends_with("</rss>"));
}

#[test]
fn channel_title_count_is_site_plus_one_per_item() {
    // <title> appears once for the channel and once per item.
    assert_eq!(seo::feed_rss().matches("<title>").count(), 1 + POSTS.len());
}
