//! Integration tests for the Atom 1.0 feed builder (`atom.xml`).

use darkrun_site::content::POSTS;
use darkrun_site::seo::{self, SITE_NAME, SITE_URL};

#[test]
fn starts_with_xml_declaration() {
    assert!(seo::feed_atom().starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
}

#[test]
fn declares_the_atom_namespace() {
    assert!(seo::feed_atom().contains("xmlns=\"http://www.w3.org/2005/Atom\""));
}

#[test]
fn has_one_feed_root() {
    let xml = seo::feed_atom();
    assert_eq!(xml.matches("<feed").count(), 1);
    assert!(xml.trim_end().ends_with("</feed>"));
}

#[test]
fn feed_carries_title_id_and_self_link() {
    let xml = seo::feed_atom();
    assert!(xml.contains(&format!("<title>{SITE_NAME}</title>")));
    assert!(xml.contains(&format!("<id>{SITE_URL}/</id>")));
    assert!(xml.contains(&format!("<link href=\"{SITE_URL}/\"/>")));
}

#[test]
fn entry_count_matches_post_count() {
    assert_eq!(seo::feed_atom().matches("<entry>").count(), POSTS.len());
}

#[test]
fn entries_are_balanced() {
    let xml = seo::feed_atom();
    assert_eq!(xml.matches("<entry>").count(), xml.matches("</entry>").count());
}

#[test]
fn every_post_has_an_entry_with_id_title_and_link() {
    let xml = seo::feed_atom();
    for post in POSTS {
        let link = format!("{SITE_URL}/blog/{}", post.slug);
        assert!(xml.contains(&format!("<title>{}</title>", post.title)));
        assert!(xml.contains(&format!("<id>{link}</id>")));
        assert!(xml.contains(&format!("<link href=\"{link}\"/>")));
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
    let xml = seo::feed_atom();
    for post in POSTS {
        assert!(xml.contains(&format!("<summary>{}</summary>", xml_escape(post.summary))));
    }
}

#[test]
fn apostrophes_in_summaries_are_escaped_to_apos() {
    // At least one post summary carries an apostrophe; it must be &apos; in XML.
    let xml = seo::feed_atom();
    if POSTS.iter().any(|p| p.summary.contains('\'')) {
        assert!(xml.contains("&apos;"));
    }
}

#[test]
fn title_count_is_feed_plus_entries() {
    assert_eq!(seo::feed_atom().matches("<title>").count(), 1 + POSTS.len());
}

#[test]
fn id_count_is_feed_plus_entries() {
    // One feed-level <id> plus one per entry.
    assert_eq!(seo::feed_atom().matches("<id>").count(), 1 + POSTS.len());
}

#[test]
fn is_deterministic() {
    assert_eq!(seo::feed_atom(), seo::feed_atom());
}

#[test]
fn entry_links_are_href_attributes_not_text() {
    // Atom uses link href="" attributes, never <link>text</link>.
    let xml = seo::feed_atom();
    assert!(!xml.contains("<link>"));
}
