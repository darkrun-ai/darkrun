//! Unit dependency-graph coverage: topological order, waves, ready-set,
//! cycle detection, self-loops, duplicate/unresolved deps, diamonds, and
//! isolated nodes.

use darkrun_core::dag::Dag;
use darkrun_core::domain::{Status, Unit, UnitFrontmatter};
use darkrun_core::error::CoreError;

fn unit(slug: &str, status: Status, deps: &[&str]) -> Unit {
    Unit {
        slug: slug.to_string(),
        frontmatter: UnitFrontmatter {
            status,
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        },
        title: slug.to_string(),
        body: String::new(),
    }
}

#[test]
fn empty_graph_is_trivially_sortable() {
    let dag = Dag::build(&[]);
    assert!(dag.nodes.is_empty());
    assert!(dag.edges.is_empty());
    assert!(dag.unresolved.is_empty());
    assert!(dag.topological_sort().expect("topo").is_empty());
    assert!(dag.waves().expect("waves").is_empty());
}

#[test]
fn single_node_no_deps() {
    let units = vec![unit("solo", Status::Pending, &[])];
    let dag = Dag::build(&units);
    assert_eq!(dag.topological_sort().expect("topo"), vec!["solo"]);
    let waves = dag.waves().expect("waves");
    assert_eq!(waves[&0], vec!["solo".to_string()]);
}

#[test]
fn isolated_nodes_share_wave_zero() {
    let units = vec![
        unit("a", Status::Pending, &[]),
        unit("b", Status::Pending, &[]),
        unit("c", Status::Pending, &[]),
    ];
    let dag = Dag::build(&units);
    let waves = dag.waves().expect("waves");
    assert_eq!(waves.len(), 1);
    assert_eq!(waves[&0], vec!["a".to_string(), "b".to_string(), "c".to_string()]);
}

#[test]
fn linear_chain_orders_and_waves() {
    let units = vec![
        unit("a", Status::Pending, &[]),
        unit("b", Status::Pending, &["a"]),
        unit("c", Status::Pending, &["b"]),
    ];
    let dag = Dag::build(&units);
    assert_eq!(dag.topological_sort().expect("topo"), vec!["a", "b", "c"]);
    let waves = dag.waves().expect("waves");
    assert_eq!(waves[&0], vec!["a".to_string()]);
    assert_eq!(waves[&1], vec!["b".to_string()]);
    assert_eq!(waves[&2], vec!["c".to_string()]);
}

#[test]
fn diamond_topology_orders_correctly() {
    let units = vec![
        unit("a", Status::Pending, &[]),
        unit("b", Status::Pending, &["a"]),
        unit("c", Status::Pending, &["a"]),
        unit("d", Status::Pending, &["b", "c"]),
    ];
    let dag = Dag::build(&units);
    let order = dag.topological_sort().expect("topo");
    let pos = |s: &str| order.iter().position(|x| x == s).unwrap();
    assert!(pos("a") < pos("b"));
    assert!(pos("a") < pos("c"));
    assert!(pos("b") < pos("d"));
    assert!(pos("c") < pos("d"));

    let waves = dag.waves().expect("waves");
    assert_eq!(waves[&0], vec!["a".to_string()]);
    assert_eq!(waves[&1], vec!["b".to_string(), "c".to_string()]);
    assert_eq!(waves[&2], vec!["d".to_string()]);
}

#[test]
fn topological_sort_is_deterministic() {
    // Two independent roots must come out sorted, not in hash order.
    let units = vec![
        unit("zebra", Status::Pending, &[]),
        unit("alpha", Status::Pending, &[]),
        unit("mango", Status::Pending, &[]),
    ];
    let dag = Dag::build(&units);
    let first = dag.topological_sort().expect("topo");
    assert_eq!(first, vec!["alpha", "mango", "zebra"]);
    // Stable across repeated runs.
    for _ in 0..20 {
        assert_eq!(dag.topological_sort().expect("topo"), first);
    }
}

#[test]
fn detects_two_node_cycle() {
    let units = vec![
        unit("a", Status::Pending, &["b"]),
        unit("b", Status::Pending, &["a"]),
    ];
    let dag = Dag::build(&units);
    let err = dag.topological_sort().unwrap_err();
    match err {
        CoreError::CyclicDependency(s) => {
            assert!(s.contains("a") && s.contains("b"));
        }
        other => panic!("expected cycle, got {other:?}"),
    }
    // Waves also surfaces the cycle.
    assert!(matches!(
        dag.waves().unwrap_err(),
        CoreError::CyclicDependency(_)
    ));
}

#[test]
fn detects_three_node_cycle() {
    let units = vec![
        unit("a", Status::Pending, &["c"]),
        unit("b", Status::Pending, &["a"]),
        unit("c", Status::Pending, &["b"]),
    ];
    let dag = Dag::build(&units);
    assert!(matches!(
        dag.topological_sort().unwrap_err(),
        CoreError::CyclicDependency(_)
    ));
}

#[test]
fn self_dependency_is_a_cycle() {
    // A unit depending on itself resolves (the slug exists) but forms a loop.
    let units = vec![unit("a", Status::Pending, &["a"])];
    let dag = Dag::build(&units);
    // The self-edge resolves, so it is not "unresolved".
    assert!(dag.unresolved.is_empty());
    assert_eq!(dag.edges.len(), 1);
    assert!(matches!(
        dag.topological_sort().unwrap_err(),
        CoreError::CyclicDependency(_)
    ));
}

#[test]
fn cycle_with_a_clean_tail_only_reports_the_cycle() {
    // `tail` sorts cleanly; a<->b cycle. Only a,b remain unsorted.
    let units = vec![
        unit("tail", Status::Pending, &[]),
        unit("a", Status::Pending, &["b"]),
        unit("b", Status::Pending, &["a"]),
    ];
    let dag = Dag::build(&units);
    match dag.topological_sort().unwrap_err() {
        CoreError::CyclicDependency(s) => {
            assert!(!s.contains("tail"), "tail is acyclic: {s}");
            assert!(s.contains("a") && s.contains("b"));
        }
        other => panic!("expected cycle, got {other:?}"),
    }
}

#[test]
fn unresolved_deps_are_collected_not_edged() {
    let units = vec![unit("a", Status::Pending, &["ghost", "phantom"])];
    let dag = Dag::build(&units);
    assert_eq!(dag.unresolved.len(), 2);
    let deps: Vec<&str> = dag.unresolved.iter().map(|u| u.dep.as_str()).collect();
    assert!(deps.contains(&"ghost"));
    assert!(deps.contains(&"phantom"));
    // No edges were produced for the missing deps.
    assert!(dag.edges.is_empty());
    // ...and the graph still sorts (a has no real predecessors).
    assert_eq!(dag.topological_sort().expect("topo"), vec!["a"]);
}

#[test]
fn unresolved_dep_records_declaring_unit() {
    let units = vec![
        unit("a", Status::Pending, &[]),
        unit("b", Status::Pending, &["nope"]),
    ];
    let dag = Dag::build(&units);
    assert_eq!(dag.unresolved.len(), 1);
    assert_eq!(dag.unresolved[0].unit, "b");
    assert_eq!(dag.unresolved[0].dep, "nope");
}

#[test]
fn duplicate_dep_yields_duplicate_edges() {
    // The builder does not dedupe; two `a` deps -> two edges.
    let units = vec![
        unit("a", Status::Completed, &[]),
        unit("b", Status::Pending, &["a", "a"]),
    ];
    let dag = Dag::build(&units);
    let a_to_b = dag
        .edges
        .iter()
        .filter(|e| e.from == "a" && e.to == "b")
        .count();
    assert_eq!(a_to_b, 2);
    // Even with the doubled edge, topo still succeeds (Kahn handles the
    // raised in-degree because each edge decrements once).
    let order = dag.topological_sort().expect("topo");
    assert_eq!(order, vec!["a", "b"]);
}

#[test]
fn ready_units_only_pending_with_completed_deps() {
    let units = vec![
        unit("a", Status::Completed, &[]),
        unit("b", Status::Pending, &["a"]),     // ready: dep complete
        unit("c", Status::Pending, &["b"]),     // not ready: b pending
        unit("d", Status::Active, &["a"]),       // not ready: not pending
        unit("e", Status::Pending, &[]),         // ready: no deps
    ];
    let dag = Dag::build(&units);
    let ready: Vec<&str> = dag
        .ready_units(&units)
        .iter()
        .map(|u| u.slug.as_str())
        .collect();
    assert_eq!(ready, vec!["b", "e"]);
}

#[test]
fn ready_units_empty_when_nothing_pending() {
    let units = vec![
        unit("a", Status::Completed, &[]),
        unit("b", Status::Active, &["a"]),
    ];
    let dag = Dag::build(&units);
    assert!(dag.ready_units(&units).is_empty());
}

#[test]
fn ready_units_blocked_by_unresolved_dep() {
    // A pending unit whose dep does not exist is never ready (dep status is
    // not Completed).
    let units = vec![unit("a", Status::Pending, &["ghost"])];
    let dag = Dag::build(&units);
    assert!(dag.ready_units(&units).is_empty());
}

#[test]
fn adjacency_lists_every_node_even_leaves() {
    let units = vec![
        unit("a", Status::Pending, &[]),
        unit("b", Status::Pending, &["a"]),
    ];
    let dag = Dag::build(&units);
    // Every node gets an adjacency entry, even leaf `b`.
    assert!(dag.adjacency.contains_key("a"));
    assert!(dag.adjacency.contains_key("b"));
    assert_eq!(dag.adjacency["a"], vec!["b".to_string()]);
    assert!(dag.adjacency["b"].is_empty());
}

#[test]
fn nodes_preserve_input_order_and_status() {
    let units = vec![
        unit("first", Status::Completed, &[]),
        unit("second", Status::Blocked, &[]),
    ];
    let dag = Dag::build(&units);
    assert_eq!(dag.nodes[0].id, "first");
    assert_eq!(dag.nodes[0].status, Status::Completed);
    assert_eq!(dag.nodes[1].id, "second");
    assert_eq!(dag.nodes[1].status, Status::Blocked);
}

#[test]
fn wide_fan_out_then_join() {
    // root -> {l1..l5} -> sink. sink depends on all five leaves.
    let mut units = vec![unit("root", Status::Pending, &[])];
    for i in 0..5 {
        let slug = format!("l{i}");
        units.push(Unit {
            slug: slug.clone(),
            frontmatter: UnitFrontmatter {
                status: Status::Pending,
                depends_on: vec!["root".into()],
                ..Default::default()
            },
            title: slug,
            body: String::new(),
        });
    }
    let leaf_slugs: Vec<String> = (0..5).map(|i| format!("l{i}")).collect();
    units.push(Unit {
        slug: "sink".into(),
        frontmatter: UnitFrontmatter {
            status: Status::Pending,
            depends_on: leaf_slugs,
            ..Default::default()
        },
        title: "sink".into(),
        body: String::new(),
    });

    let dag = Dag::build(&units);
    let waves = dag.waves().expect("waves");
    assert_eq!(waves[&0], vec!["root".to_string()]);
    assert_eq!(waves[&1].len(), 5);
    assert_eq!(waves[&2], vec!["sink".to_string()]);
}
