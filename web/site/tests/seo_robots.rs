//! Integration tests for the robots.txt builder in `darkrun_site::seo`.

use darkrun_site::seo::{self, SITE_URL};

#[test]
fn starts_with_a_comment_header() {
    assert!(seo::robots().starts_with("# darkrun robots.txt"));
}

#[test]
fn allows_all_user_agents() {
    let txt = seo::robots();
    assert!(txt.contains("User-agent: *"));
    assert!(txt.contains("Allow: /"));
}

#[test]
fn points_at_the_sitemap_with_an_absolute_url() {
    assert!(seo::robots().contains(&format!("Sitemap: {SITE_URL}/sitemap.xml")));
}

#[test]
fn welcomes_named_ai_crawlers() {
    let txt = seo::robots();
    for bot in [
        "GPTBot",
        "ClaudeBot",
        "anthropic-ai",
        "Google-Extended",
        "PerplexityBot",
        "CCBot",
    ] {
        assert!(txt.contains(bot), "missing crawler {bot}");
    }
}

#[test]
fn every_named_crawler_is_explicitly_allowed() {
    // Each `User-agent: <bot>` line is immediately followed by an `Allow: /`.
    let txt = seo::robots();
    for bot in ["GPTBot", "ClaudeBot", "anthropic-ai", "Google-Extended", "PerplexityBot", "CCBot"] {
        let marker = format!("User-agent: {bot}\nAllow: /");
        assert!(txt.contains(&marker), "crawler {bot} not paired with Allow: /");
    }
}

#[test]
fn references_all_three_feed_urls() {
    let txt = seo::robots();
    assert!(txt.contains(&format!("{SITE_URL}/feed.xml")));
    assert!(txt.contains(&format!("{SITE_URL}/atom.xml")));
    assert!(txt.contains(&format!("{SITE_URL}/feed.json")));
}

#[test]
fn never_disallows_anything() {
    assert!(!seo::robots().contains("Disallow"));
}

#[test]
fn is_deterministic() {
    assert_eq!(seo::robots(), seo::robots());
}

#[test]
fn ends_with_a_newline() {
    assert!(seo::robots().ends_with('\n'));
}

#[test]
fn contains_exactly_one_sitemap_directive() {
    assert_eq!(seo::robots().matches("Sitemap:").count(), 1);
}

#[test]
fn has_no_blank_user_agent_lines() {
    for line in seo::robots().lines() {
        if let Some(rest) = line.strip_prefix("User-agent: ") {
            assert!(!rest.trim().is_empty(), "blank user-agent line");
        }
    }
}
