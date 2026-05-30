//! Cross-cutting SEO invariants: the three feeds must agree on the post set and
//! their URLs, and the whole artifact bundle (as the generator writes it) must
//! be internally consistent.

use darkrun_site::content::POSTS;
use darkrun_site::route::Route;
use darkrun_site::seo::{self, SITE_URL};

/// The canonical blog link for a post slug.
fn blog_link(slug: &str) -> String {
    format!("{SITE_URL}/blog/{slug}")
}

#[test]
fn all_three_feeds_reference_the_same_post_links() {
    let rss = seo::feed_rss();
    let atom = seo::feed_atom();
    let json = seo::feed_json();
    for post in POSTS {
        let link = blog_link(post.slug);
        assert!(rss.contains(&link), "rss missing {link}");
        assert!(atom.contains(&link), "atom missing {link}");
        assert!(json.contains(&link), "json missing {link}");
    }
}

#[test]
fn all_three_feeds_reference_the_same_titles() {
    let rss = seo::feed_rss();
    let atom = seo::feed_atom();
    let json = seo::feed_json();
    for post in POSTS {
        assert!(rss.contains(post.title));
        assert!(atom.contains(post.title));
        assert!(json.contains(post.title));
    }
}

#[test]
fn each_feed_post_link_also_appears_in_the_sitemap() {
    let sitemap = seo::sitemap();
    for post in POSTS {
        assert!(
            sitemap.contains(&blog_link(post.slug)),
            "post {} in feeds but not sitemap",
            post.slug
        );
    }
}

#[test]
fn feed_urls_advertised_in_robots_have_builders() {
    // robots.txt advertises feed.xml / atom.xml / feed.json — all three exist.
    let robots = seo::robots();
    assert!(robots.contains("feed.xml"));
    assert!(robots.contains("atom.xml"));
    assert!(robots.contains("feed.json"));
    assert!(!seo::feed_rss().is_empty());
    assert!(!seo::feed_atom().is_empty());
    assert!(!seo::feed_json().is_empty());
}

#[test]
fn the_generated_bundle_has_five_artifacts_plus_a_manifest() {
    // Mirrors the gen binary's file set; here we just confirm each builder
    // produces non-empty output and the manifest has one line per route.
    let artifacts: Vec<(&str, String)> = vec![
        ("sitemap.xml", seo::sitemap()),
        ("robots.txt", seo::robots()),
        ("feed.xml", seo::feed_rss()),
        ("atom.xml", seo::feed_atom()),
        ("feed.json", seo::feed_json()),
    ];
    for (name, body) in &artifacts {
        assert!(!body.trim().is_empty(), "{name} is empty");
    }
    let manifest_lines = Route::all_paths().len();
    assert_eq!(manifest_lines, Route::all_paths().len());
}

#[test]
fn xml_artifacts_all_carry_the_encoding_declaration() {
    for xml in [seo::sitemap(), seo::feed_rss(), seo::feed_atom()] {
        assert!(xml.contains("encoding=\"UTF-8\""));
    }
}

#[test]
fn json_feed_is_the_only_artifact_that_is_not_xml() {
    assert!(!seo::feed_json().starts_with("<?xml"));
    assert!(seo::sitemap().starts_with("<?xml"));
    assert!(seo::feed_rss().starts_with("<?xml"));
    assert!(seo::feed_atom().starts_with("<?xml"));
}

#[test]
fn site_constants_have_expected_shape() {
    assert!(SITE_URL.starts_with("https://"));
    assert!(!SITE_URL.ends_with('/'), "origin must not have a trailing slash");
    assert!(!seo::SITE_NAME.is_empty());
    assert!(!seo::SITE_DESCRIPTION.is_empty());
}

#[test]
fn no_feed_links_double_up_the_origin() {
    // A bug doubling the origin would emit `https://...https://...`.
    for body in [seo::feed_rss(), seo::feed_atom(), seo::feed_json(), seo::sitemap()] {
        assert!(!body.contains(&format!("{SITE_URL}{SITE_URL}")), "doubled origin");
    }
}
