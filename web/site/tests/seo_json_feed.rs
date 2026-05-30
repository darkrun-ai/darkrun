//! Integration tests for the JSON Feed 1.1 builder (`feed.json`).
//!
//! No JSON dependency is pulled in; structure is asserted directly and a small
//! brace/quote balance check stands in for full validity.

use darkrun_site::content::POSTS;
use darkrun_site::seo::{self, SITE_DESCRIPTION, SITE_NAME, SITE_URL};

#[test]
fn declares_the_json_feed_version() {
    assert!(seo::feed_json().contains("\"version\":\"https://jsonfeed.org/version/1.1\""));
}

#[test]
fn carries_top_level_metadata() {
    let json = seo::feed_json();
    assert!(json.contains(&format!("\"title\":\"{SITE_NAME}\"")));
    assert!(json.contains(&format!("\"home_page_url\":\"{SITE_URL}\"")));
    assert!(json.contains(&format!("\"feed_url\":\"{SITE_URL}/feed.json\"")));
    assert!(json.contains(&format!("\"description\":\"{SITE_DESCRIPTION}\"")));
}

#[test]
fn has_an_items_array() {
    let json = seo::feed_json();
    assert!(json.contains("\"items\":["));
    assert!(json.trim_end().ends_with("]}"));
}

#[test]
fn one_item_object_per_post() {
    // Each item carries exactly one `"id":` key.
    assert_eq!(seo::feed_json().matches("\"id\":").count(), POSTS.len());
}

#[test]
fn every_post_id_and_url_are_the_blog_link() {
    let json = seo::feed_json();
    for post in POSTS {
        let link = format!("{SITE_URL}/blog/{}", post.slug);
        assert!(json.contains(&format!("\"id\":\"{link}\"")), "id {link}");
        assert!(json.contains(&format!("\"url\":\"{link}\"")), "url {link}");
    }
}

#[test]
fn every_post_title_and_summary_present() {
    let json = seo::feed_json();
    for post in POSTS {
        assert!(json.contains(&format!("\"title\":\"{}\"", post.title)));
        assert!(json.contains(&format!("\"summary\":\"{}\"", post.summary)));
    }
}

#[test]
fn items_are_comma_separated() {
    // N items => N-1 commas between them inside the array.
    if POSTS.len() >= 2 {
        let json = seo::feed_json();
        let items = &json[json.find("\"items\":[").unwrap()..];
        // "},{" joins adjacent item objects.
        assert_eq!(items.matches("},{").count(), POSTS.len() - 1);
    }
}

#[test]
fn braces_are_balanced() {
    let json = seo::feed_json();
    let open = json.matches('{').count();
    let close = json.matches('}').count();
    assert_eq!(open, close, "unbalanced braces");
}

#[test]
fn brackets_are_balanced() {
    let json = seo::feed_json();
    assert_eq!(json.matches('[').count(), json.matches(']').count());
}

#[test]
fn quotes_are_even() {
    // No control chars or stray quotes in our corpus, so quote count is even.
    assert_eq!(seo::feed_json().matches('"').count() % 2, 0);
}

#[test]
fn item_order_matches_posts_order() {
    let json = seo::feed_json();
    let mut last = 0usize;
    for post in POSTS {
        let link = format!("{SITE_URL}/blog/{}", post.slug);
        let needle = format!("\"id\":\"{link}\"");
        let at = json.find(&needle).expect("item present");
        assert!(at >= last, "item out of order: {link}");
        last = at;
    }
}

#[test]
fn is_deterministic() {
    assert_eq!(seo::feed_json(), seo::feed_json());
}

#[test]
fn single_top_level_object() {
    let json = seo::feed_json();
    assert!(json.starts_with('{'));
    assert!(json.trim_end().ends_with('}'));
}
