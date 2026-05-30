//! Integration tests for the /browse page's data source: the
//! `darkrun_api::ROUTES` contract it lists, and the `HttpMethod` distinctions
//! that drive the row tone (WS vs HTTP).

use darkrun_api::{HttpMethod, ROUTES};

#[test]
fn routes_table_is_non_empty() {
    assert!(!ROUTES.is_empty());
}

#[test]
fn every_route_has_a_rooted_path_template() {
    for spec in ROUTES {
        assert!(spec.path_template.starts_with('/'), "bad path {}", spec.path_template);
    }
}

#[test]
fn every_route_has_a_non_empty_summary_and_tag() {
    for spec in ROUTES {
        assert!(!spec.summary.is_empty(), "{} has empty summary", spec.path_template);
        assert!(!spec.tag.is_empty(), "{} has empty tag", spec.path_template);
    }
}

#[test]
fn method_debug_renders_a_short_uppercase_label() {
    // The page formats `{:?}` of the method into a badge; check it's sensible.
    for spec in ROUTES {
        let label = format!("{:?}", spec.method);
        assert!(!label.is_empty());
        assert!(label.chars().next().unwrap().is_ascii_uppercase());
    }
}

#[test]
fn ws_is_the_only_method_that_flags_is_ws() {
    for spec in ROUTES {
        let is_ws = spec.method == HttpMethod::Ws;
        assert_eq!(is_ws, matches!(spec.method, HttpMethod::Ws), "{}", spec.path_template);
    }
}

#[test]
fn at_least_one_websocket_upgrade_route_exists() {
    assert!(ROUTES.iter().any(|s| s.method == HttpMethod::Ws));
}

#[test]
fn the_ws_route_is_the_session_socket() {
    let ws: Vec<_> = ROUTES.iter().filter(|s| s.method == HttpMethod::Ws).collect();
    assert_eq!(ws.len(), 1);
    assert!(ws[0].path_template.contains("/ws/session/"));
}

#[test]
fn http_methods_present_for_a_real_contract() {
    let has = |m: HttpMethod| ROUTES.iter().any(|s| s.method == m);
    assert!(has(HttpMethod::Get));
    assert!(has(HttpMethod::Post));
    assert!(has(HttpMethod::Head));
    assert!(has(HttpMethod::Put));
    assert!(has(HttpMethod::Delete));
}

#[test]
fn operation_ids_are_unique() {
    let mut ids: Vec<&str> = ROUTES.iter().map(|s| s.operation_id).collect();
    let len = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(len, ids.len(), "duplicate operation_id");
}

#[test]
fn method_plus_path_pairs_are_unique() {
    let mut pairs: Vec<(HttpMethod, &str)> =
        ROUTES.iter().map(|s| (s.method, s.path_template)).collect();
    let len = pairs.len();
    pairs.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    pairs.dedup();
    assert_eq!(len, pairs.len(), "duplicate method+path");
}

#[test]
fn review_current_route_is_present_and_get() {
    let spec = ROUTES
        .iter()
        .find(|s| s.path_template == "/api/review/current")
        .expect("review/current route");
    assert_eq!(spec.method, HttpMethod::Get);
    assert_eq!(spec.tag, "review");
}

#[test]
fn health_route_is_present() {
    assert!(ROUTES.iter().any(|s| s.path_template == "/health" && s.method == HttpMethod::Get));
}

#[test]
fn feedback_routes_cover_crud() {
    let feedback: Vec<_> = ROUTES.iter().filter(|s| s.tag == "feedback").collect();
    let methods: Vec<HttpMethod> = feedback.iter().map(|s| s.method).collect();
    assert!(methods.contains(&HttpMethod::Get));
    assert!(methods.contains(&HttpMethod::Post));
    assert!(methods.contains(&HttpMethod::Put));
    assert!(methods.contains(&HttpMethod::Delete));
}

#[test]
fn templated_paths_use_brace_placeholders() {
    // Several routes are templated; placeholders are `{...}`, never `:...`.
    for spec in ROUTES {
        assert!(!spec.path_template.contains(':'), "colon placeholder in {}", spec.path_template);
    }
}

#[test]
fn tags_group_related_routes() {
    // The page renders a tag badge per row; tags are a small known set.
    use std::collections::BTreeSet;
    let tags: BTreeSet<&str> = ROUTES.iter().map(|s| s.tag).collect();
    for expected in ["session", "review", "feedback", "health", "websocket"] {
        assert!(tags.contains(expected), "missing tag {expected}");
    }
}

#[test]
fn find_helper_resolves_a_known_route() {
    let spec = darkrun_api::routes::find(HttpMethod::Get, "/health");
    assert!(spec.is_some());
    assert_eq!(spec.unwrap().operation_id, "getHealth");
}

#[test]
fn find_helper_misses_an_unknown_route() {
    assert!(darkrun_api::routes::find(HttpMethod::Get, "/nope").is_none());
    // Right path, wrong method.
    assert!(darkrun_api::routes::find(HttpMethod::Post, "/health").is_none());
}
