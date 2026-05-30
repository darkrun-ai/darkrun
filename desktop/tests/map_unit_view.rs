//! `unit_view`, `extract_criteria`, and `first_str` flattening of the opaque
//! unit `Value`. Probes every conventional key, missing fields, wrong types,
//! nested shapes, and graceful defaults.

use darkrun_desktop::map::{extract_criteria, first_str, unit_view};
use darkrun_ui::kinds::Tone;
use serde_json::json;

// ---- first_str: key precedence + type filtering ----

#[test]
fn first_str_picks_first_present_key_in_order() {
    let v = json!({ "name": "b", "title": "a" });
    // `title` listed first wins even though `name` is also present.
    assert_eq!(first_str(&v, &["title", "name"]).as_deref(), Some("a"));
    // Reverse the order: `name` now wins.
    assert_eq!(first_str(&v, &["name", "title"]).as_deref(), Some("b"));
}

#[test]
fn first_str_skips_missing_keys() {
    let v = json!({ "slug": "only" });
    assert_eq!(
        first_str(&v, &["title", "name", "slug"]).as_deref(),
        Some("only")
    );
}

#[test]
fn first_str_none_when_no_keys_present() {
    let v = json!({ "other": "x" });
    assert_eq!(first_str(&v, &["title", "name"]), None);
}

#[test]
fn first_str_none_for_empty_object() {
    assert_eq!(first_str(&json!({}), &["title"]), None);
}

#[test]
fn first_str_skips_non_string_values() {
    // A numeric `title` is not a string; should fall through to `name`.
    let v = json!({ "title": 42, "name": "fallback" });
    assert_eq!(first_str(&v, &["title", "name"]).as_deref(), Some("fallback"));
}

#[test]
fn first_str_skips_null_and_bool_and_object() {
    let v = json!({ "a": null, "b": true, "c": {"x": 1}, "d": "got it" });
    assert_eq!(first_str(&v, &["a", "b", "c", "d"]).as_deref(), Some("got it"));
}

#[test]
fn first_str_empty_string_is_still_a_string() {
    // An empty string is a valid string; precedence does not skip it.
    let v = json!({ "title": "", "name": "real" });
    assert_eq!(first_str(&v, &["title", "name"]).as_deref(), Some(""));
}

#[test]
fn first_str_on_non_object_value_returns_none() {
    assert_eq!(first_str(&json!("plain"), &["title"]), None);
    assert_eq!(first_str(&json!([1, 2, 3]), &["title"]), None);
    assert_eq!(first_str(&json!(7), &["title"]), None);
}

#[test]
fn first_str_empty_key_list_is_none() {
    assert_eq!(first_str(&json!({ "title": "x" }), &[]), None);
}

// ---- unit_view: title resolution ----

#[test]
fn title_from_title_key() {
    assert_eq!(unit_view(&json!({ "title": "T" })).title, "T");
}

#[test]
fn title_from_name_when_no_title() {
    assert_eq!(unit_view(&json!({ "name": "N" })).title, "N");
}

#[test]
fn title_from_slug_when_no_title_or_name() {
    assert_eq!(unit_view(&json!({ "slug": "the-slug" })).title, "the-slug");
}

#[test]
fn title_from_id_when_only_id() {
    assert_eq!(unit_view(&json!({ "id": "u-99" })).title, "u-99");
}

#[test]
fn title_precedence_title_over_name_over_slug_over_id() {
    let v = json!({ "id": "i", "slug": "s", "name": "n", "title": "t" });
    assert_eq!(unit_view(&v).title, "t");
    let v = json!({ "id": "i", "slug": "s", "name": "n" });
    assert_eq!(unit_view(&v).title, "n");
    let v = json!({ "id": "i", "slug": "s" });
    assert_eq!(unit_view(&v).title, "s");
    let v = json!({ "id": "i" });
    assert_eq!(unit_view(&v).title, "i");
}

#[test]
fn title_defaults_to_unit_when_absent() {
    assert_eq!(unit_view(&json!({})).title, "unit");
}

#[test]
fn title_default_when_keys_are_wrong_type() {
    // None of the title keys are strings -> default.
    let v = json!({ "title": 1, "name": [], "slug": false, "id": null });
    assert_eq!(unit_view(&v).title, "unit");
}

// ---- unit_view: unit_type ----

#[test]
fn unit_type_from_unit_type_key() {
    assert_eq!(
        unit_view(&json!({ "unit_type": "epic" })).unit_type.as_deref(),
        Some("epic")
    );
}

#[test]
fn unit_type_from_type_key() {
    assert_eq!(
        unit_view(&json!({ "type": "feature" })).unit_type.as_deref(),
        Some("feature")
    );
}

#[test]
fn unit_type_from_kind_key() {
    assert_eq!(
        unit_view(&json!({ "kind": "bug" })).unit_type.as_deref(),
        Some("bug")
    );
}

#[test]
fn unit_type_precedence() {
    let v = json!({ "kind": "k", "type": "t", "unit_type": "ut" });
    assert_eq!(unit_view(&v).unit_type.as_deref(), Some("ut"));
    let v = json!({ "kind": "k", "type": "t" });
    assert_eq!(unit_view(&v).unit_type.as_deref(), Some("t"));
}

#[test]
fn unit_type_none_when_absent() {
    assert_eq!(unit_view(&json!({ "title": "x" })).unit_type, None);
}

#[test]
fn unit_type_none_when_wrong_type() {
    assert_eq!(unit_view(&json!({ "type": 5 })).unit_type, None);
}

// ---- unit_view: status_label + tone ----

#[test]
fn status_from_status_key_lowercased() {
    let v = unit_view(&json!({ "status": "ACTIVE" }));
    assert_eq!(v.status_label, "active");
    assert_eq!(v.tone, Tone::Info);
}

#[test]
fn status_from_state_key() {
    let v = unit_view(&json!({ "state": "Done" }));
    assert_eq!(v.status_label, "done");
    assert_eq!(v.tone, Tone::Ok);
}

#[test]
fn status_precedence_status_over_state() {
    let v = unit_view(&json!({ "state": "failed", "status": "passed" }));
    assert_eq!(v.status_label, "passed");
    assert_eq!(v.tone, Tone::Ok);
}

#[test]
fn status_defaults_to_pending() {
    let v = unit_view(&json!({}));
    assert_eq!(v.status_label, "pending");
    assert_eq!(v.tone, Tone::Warn);
}

#[test]
fn status_default_when_wrong_type() {
    let v = unit_view(&json!({ "status": 123 }));
    assert_eq!(v.status_label, "pending");
    assert_eq!(v.tone, Tone::Warn);
}

#[test]
fn status_unknown_token_is_neutral() {
    let v = unit_view(&json!({ "status": "frobnicated" }));
    assert_eq!(v.status_label, "frobnicated");
    assert_eq!(v.tone, Tone::Neutral);
}

#[test]
fn status_label_preserves_unknown_casing_lowercased() {
    let v = unit_view(&json!({ "status": "WeirdThing" }));
    assert_eq!(v.status_label, "weirdthing");
}

// ---- unit_view: pass counter ----

#[test]
fn pass_from_pass_key() {
    assert_eq!(unit_view(&json!({ "pass": 3 })).pass, 3);
}

#[test]
fn pass_from_passes_key() {
    assert_eq!(unit_view(&json!({ "passes": 7 })).pass, 7);
}

#[test]
fn pass_from_visit_key() {
    assert_eq!(unit_view(&json!({ "visit": 2 })).pass, 2);
}

#[test]
fn pass_precedence_pass_over_passes_over_visit() {
    let v = json!({ "visit": 1, "passes": 2, "pass": 9 });
    assert_eq!(unit_view(&v).pass, 9);
    let v = json!({ "visit": 1, "passes": 2 });
    assert_eq!(unit_view(&v).pass, 2);
    let v = json!({ "visit": 1 });
    assert_eq!(unit_view(&v).pass, 1);
}

#[test]
fn pass_defaults_to_zero() {
    assert_eq!(unit_view(&json!({})).pass, 0);
}

#[test]
fn pass_zero_explicit() {
    assert_eq!(unit_view(&json!({ "pass": 0 })).pass, 0);
}

#[test]
fn pass_ignores_negative_and_float_and_string() {
    // as_u64 rejects negatives, floats, and strings -> default 0.
    assert_eq!(unit_view(&json!({ "pass": -1 })).pass, 0);
    assert_eq!(unit_view(&json!({ "pass": 2.5 })).pass, 0);
    assert_eq!(unit_view(&json!({ "pass": "3" })).pass, 0);
}

#[test]
fn pass_large_value_truncates_via_as_u32() {
    // u64 above u32::MAX truncates on the `as u32` cast.
    let v = unit_view(&json!({ "pass": (u32::MAX as u64) + 1 }));
    assert_eq!(v.pass, 0);
}

#[test]
fn pass_u32_max_preserved() {
    let v = unit_view(&json!({ "pass": u32::MAX as u64 }));
    assert_eq!(v.pass, u32::MAX);
}

// ---- extract_criteria: shapes + keys ----

#[test]
fn criteria_list_of_strings() {
    let v = json!({ "criteria": ["a", "b", "c"] });
    assert_eq!(extract_criteria(&v), vec!["a", "b", "c"]);
}

#[test]
fn criteria_from_completion_criteria_key() {
    let v = json!({ "completion_criteria": ["x"] });
    assert_eq!(extract_criteria(&v), vec!["x"]);
}

#[test]
fn criteria_from_acceptance_key() {
    let v = json!({ "acceptance": ["accept me"] });
    assert_eq!(extract_criteria(&v), vec!["accept me"]);
}

#[test]
fn criteria_from_checks_key() {
    let v = json!({ "checks": ["ci green"] });
    assert_eq!(extract_criteria(&v), vec!["ci green"]);
}

#[test]
fn criteria_key_precedence_first_nonempty_wins() {
    // `criteria` is probed first; even though others exist, it wins.
    let v = json!({
        "checks": ["from checks"],
        "criteria": ["from criteria"],
    });
    assert_eq!(extract_criteria(&v), vec!["from criteria"]);
}

#[test]
fn criteria_empty_array_falls_through_to_next_key() {
    // An empty `criteria` yields no lines, so the loop moves to `checks`.
    let v = json!({
        "criteria": [],
        "checks": ["fallback line"],
    });
    assert_eq!(extract_criteria(&v), vec!["fallback line"]);
}

#[test]
fn criteria_objects_with_text_field() {
    let v = json!({ "criteria": [{ "text": "one" }, { "text": "two" }] });
    assert_eq!(extract_criteria(&v), vec!["one", "two"]);
}

#[test]
fn criteria_objects_with_description_label_name_criterion() {
    let v = json!({ "criteria": [
        { "description": "desc" },
        { "label": "lbl" },
        { "name": "nm" },
        { "criterion": "crit" },
    ]});
    assert_eq!(extract_criteria(&v), vec!["desc", "lbl", "nm", "crit"]);
}

#[test]
fn criteria_object_field_precedence() {
    // text > description > label > name > criterion.
    let v = json!({ "criteria": [
        { "criterion": "c", "name": "n", "label": "l", "description": "d", "text": "t" },
    ]});
    assert_eq!(extract_criteria(&v), vec!["t"]);
}

#[test]
fn criteria_mixed_strings_and_objects() {
    let v = json!({ "criteria": ["plain", { "text": "obj" }] });
    assert_eq!(extract_criteria(&v), vec!["plain", "obj"]);
}

#[test]
fn criteria_drops_blank_strings() {
    let v = json!({ "criteria": ["keep", "", "   ", "\t", "also"] });
    assert_eq!(extract_criteria(&v), vec!["keep", "also"]);
}

#[test]
fn criteria_drops_objects_without_known_field() {
    let v = json!({ "criteria": [{ "wat": "x" }, { "text": "kept" }] });
    assert_eq!(extract_criteria(&v), vec!["kept"]);
}

#[test]
fn criteria_drops_non_string_non_object_items() {
    let v = json!({ "criteria": [1, true, null, ["nested"], "survivor"] });
    assert_eq!(extract_criteria(&v), vec!["survivor"]);
}

#[test]
fn criteria_object_with_blank_text_is_dropped() {
    let v = json!({ "criteria": [{ "text": "   " }, { "text": "real" }] });
    assert_eq!(extract_criteria(&v), vec!["real"]);
}

#[test]
fn criteria_all_blank_yields_empty_and_does_not_fall_through() {
    // The `criteria` array is present and an array, but every line filters out.
    // Result is empty for that key; the loop then tries the remaining keys.
    let v = json!({
        "criteria": ["", "  "],
        "checks": ["backup"],
    });
    assert_eq!(extract_criteria(&v), vec!["backup"]);
}

#[test]
fn criteria_absent_is_empty() {
    assert_eq!(extract_criteria(&json!({})), Vec::<String>::new());
}

#[test]
fn criteria_wrong_type_for_key_is_ignored() {
    // `criteria` is a string, not an array -> ignored, fall through.
    let v = json!({ "criteria": "not an array", "checks": ["ok"] });
    assert_eq!(extract_criteria(&v), vec!["ok"]);
}

#[test]
fn criteria_object_value_for_key_is_ignored() {
    let v = json!({ "criteria": { "text": "obj-not-array" } });
    assert_eq!(extract_criteria(&v), Vec::<String>::new());
}

#[test]
fn criteria_preserves_order_and_duplicates() {
    let v = json!({ "criteria": ["a", "a", "b", "a"] });
    assert_eq!(extract_criteria(&v), vec!["a", "a", "b", "a"]);
}

#[test]
fn criteria_large_input() {
    let lines: Vec<String> = (0..1000).map(|i| format!("line {i}")).collect();
    let v = json!({ "criteria": lines });
    let out = extract_criteria(&v);
    assert_eq!(out.len(), 1000);
    assert_eq!(out[0], "line 0");
    assert_eq!(out[999], "line 999");
}

#[test]
fn criteria_unicode_preserved() {
    let v = json!({ "criteria": ["café ☕", "日本語", "emoji 🚀"] });
    assert_eq!(extract_criteria(&v), vec!["café ☕", "日本語", "emoji 🚀"]);
}

// ---- unit_view: end-to-end shapes ----

#[test]
fn full_unit_view_all_fields() {
    let v = json!({
        "title": "Wire the importer",
        "type": "feature",
        "status": "Active",
        "pass": 2,
        "criteria": ["builds", { "text": "tests pass" }],
    });
    let view = unit_view(&v);
    assert_eq!(view.title, "Wire the importer");
    assert_eq!(view.unit_type.as_deref(), Some("feature"));
    assert_eq!(view.status_label, "active");
    assert_eq!(view.tone, Tone::Info);
    assert_eq!(view.pass, 2);
    assert_eq!(view.criteria, vec!["builds", "tests pass"]);
}

#[test]
fn empty_object_yields_all_defaults() {
    let view = unit_view(&json!({}));
    assert_eq!(view.title, "unit");
    assert_eq!(view.unit_type, None);
    assert_eq!(view.status_label, "pending");
    assert_eq!(view.tone, Tone::Warn);
    assert_eq!(view.pass, 0);
    assert!(view.criteria.is_empty());
}

#[test]
fn unit_view_default_struct_matches_empty_probe_shape() {
    use darkrun_desktop::map::UnitView;
    let d = UnitView::default();
    // The Default derive zeroes everything; unit_view on {} differs only in the
    // conventional string defaults it injects.
    assert_eq!(d.title, "");
    assert_eq!(d.pass, 0);
    // `UnitView` derives Default; its `Tone` field takes the enum's default
    // (`Accent`), distinct from the `Neutral` an unknown status label produces.
    assert_eq!(d.tone, Tone::default());
    assert_eq!(d.tone, Tone::Accent);
    assert!(d.criteria.is_empty());

    let probed = unit_view(&json!({}));
    assert_ne!(probed, d); // title/status defaults differ from zero-value
    assert_eq!(probed.title, "unit");
    assert_eq!(probed.status_label, "pending");
}

#[test]
fn unit_view_on_non_object_value_is_all_defaults() {
    // A bare array/string isn't an object; every probe misses.
    let view = unit_view(&json!(["not", "an", "object"]));
    assert_eq!(view.title, "unit");
    assert_eq!(view.status_label, "pending");
    assert_eq!(view.pass, 0);
    assert!(view.criteria.is_empty());
}

#[test]
fn unit_view_is_cloneable_and_eq() {
    let a = unit_view(&json!({ "title": "x", "pass": 1 }));
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn unit_view_nested_criteria_objects_with_extra_noise() {
    let v = json!({
        "title": "noisy",
        "completion_criteria": [
            { "text": "real", "done": true, "weight": 0.5 },
            { "id": 9, "label": "from-label" },
            { "unrelated": { "deep": "nope" } },
        ],
    });
    let view = unit_view(&v);
    assert_eq!(view.criteria, vec!["real", "from-label"]);
}
