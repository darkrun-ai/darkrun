//! Integration tests for the sitemap.xml builder in `darkrun_site::seo`.
//!
//! These exercise XML well-formedness, route coverage, the origin prefix on
//! every `<loc>`, dynamic factory/doc/post expansion, escaping, and ordering.

use darkrun_site::route::Route;
use darkrun_site::seo::{self, SITE_URL};

/// Pull the body of every `<loc>...</loc>` element out of the sitemap.
fn locs(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<loc>") {
        let after = &rest[start + "<loc>".len()..];
        let end = after.find("</loc>").expect("every <loc> is closed");
        out.push(after[..end].to_string());
        rest = &after[end + "</loc>".len()..];
    }
    out
}

#[test]
fn starts_with_xml_declaration() {
    assert!(seo::sitemap().starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
}

#[test]
fn declares_the_sitemap_namespace() {
    assert!(seo::sitemap().contains("xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\""));
}

#[test]
fn opens_and_closes_the_urlset() {
    let xml = seo::sitemap();
    assert!(xml.contains("<urlset"));
    assert!(xml.trim_end().ends_with("</urlset>"));
}

#[test]
fn loc_and_url_tags_are_balanced() {
    let xml = seo::sitemap();
    assert_eq!(xml.matches("<url>").count(), xml.matches("</url>").count());
    assert_eq!(xml.matches("<loc>").count(), xml.matches("</loc>").count());
}

#[test]
fn one_url_element_per_path() {
    let xml = seo::sitemap();
    assert_eq!(xml.matches("<url>").count(), Route::all_paths().len());
    assert_eq!(locs(&xml).len(), Route::all_paths().len());
}

#[test]
fn every_loc_is_absolute_under_the_site_origin() {
    for loc in locs(&seo::sitemap()) {
        assert!(loc.starts_with(SITE_URL), "loc not under origin: {loc}");
    }
}

#[test]
fn every_loc_has_a_rooted_path_after_the_origin() {
    for loc in locs(&seo::sitemap()) {
        let path = &loc[SITE_URL.len()..];
        assert!(path.starts_with('/'), "path not rooted: {loc}");
    }
}

#[test]
fn lists_the_landing_root() {
    assert!(seo::sitemap().contains(&format!("<loc>{SITE_URL}/</loc>")));
}

#[test]
fn lists_every_static_section() {
    let xml = seo::sitemap();
    for section in [
        "/factories",
        "/docs",
        "/methodology",
        "/glossary",
        "/lifecycles",
        "/blog",
        "/changelog",
        "/paper",
        "/templates",
        "/browse",
        "/review",
        "/privacy",
        "/terms",
    ] {
        assert!(
            xml.contains(&format!("<loc>{SITE_URL}{section}</loc>")),
            "missing section {section}"
        );
    }
}

#[test]
fn expands_dynamic_factory_routes() {
    let xml = seo::sitemap();
    assert!(xml.contains(&format!("<loc>{SITE_URL}/factories/software</loc>")));
}

#[test]
fn includes_every_embedded_factory() {
    let xml = seo::sitemap();
    for slug in darkrun_content::list_factories() {
        assert!(
            xml.contains(&format!("<loc>{SITE_URL}/factories/{slug}</loc>")),
            "missing factory {slug}"
        );
    }
}

#[test]
fn includes_every_doc_slug() {
    let xml = seo::sitemap();
    for doc in darkrun_site::content::DOCS {
        assert!(
            xml.contains(&format!("<loc>{SITE_URL}/docs/{}</loc>", doc.slug)),
            "missing doc {}",
            doc.slug
        );
    }
}

#[test]
fn includes_every_post_slug() {
    let xml = seo::sitemap();
    for post in darkrun_site::content::POSTS {
        assert!(
            xml.contains(&format!("<loc>{SITE_URL}/blog/{}</loc>", post.slug)),
            "missing post {}",
            post.slug
        );
    }
}

#[test]
fn loc_set_matches_all_paths_exactly() {
    let mut from_xml = locs(&seo::sitemap());
    let mut from_routes: Vec<String> =
        Route::all_paths().into_iter().map(|p| format!("{SITE_URL}{p}")).collect();
    from_xml.sort();
    from_routes.sort();
    assert_eq!(from_xml, from_routes);
}

#[test]
fn ordering_follows_all_paths() {
    // The sitemap must emit URLs in the same nav order all_paths declares.
    let from_xml = locs(&seo::sitemap());
    let from_routes: Vec<String> =
        Route::all_paths().into_iter().map(|p| format!("{SITE_URL}{p}")).collect();
    assert_eq!(from_xml, from_routes);
}

#[test]
fn is_deterministic_across_calls() {
    assert_eq!(seo::sitemap(), seo::sitemap());
}

#[test]
fn no_raw_unescaped_ampersand_in_locs() {
    // A bare `&` (not part of an entity) would be invalid XML.
    for loc in locs(&seo::sitemap()) {
        let mut rest = loc.as_str();
        while let Some(i) = rest.find('&') {
            let tail = &rest[i..];
            assert!(
                tail.starts_with("&amp;")
                    || tail.starts_with("&lt;")
                    || tail.starts_with("&gt;")
                    || tail.starts_with("&quot;")
                    || tail.starts_with("&apos;"),
                "bare ampersand in {loc}"
            );
            rest = &tail[1..];
        }
    }
}

#[test]
fn no_factory_index_double_listing() {
    // `/factories` (index) and `/factories/<slug>` (detail) are distinct.
    let xml = seo::sitemap();
    assert!(xml.contains(&format!("<loc>{SITE_URL}/factories</loc>")));
    assert!(xml.contains(&format!("<loc>{SITE_URL}/factories/software</loc>")));
}

#[test]
fn no_trailing_whitespace_lines_break_structure() {
    // Sanity: the document round-trips through trimming without losing the root.
    let xml = seo::sitemap();
    assert!(xml.trim().starts_with("<?xml"));
    assert!(xml.trim().ends_with("</urlset>"));
}
