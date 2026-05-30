//! Comprehensive dependency-graph coverage for darkrun-core's `dag` module.
//!
//! Drives the public surface: `Dag::build`, `topological_sort`, `waves`, and
//! `ready_units`. Exercises linear / diamond / wide / deep / random-but-
//! deterministic graphs, every flavour of cycle (self, two-node, long chain,
//! multiple disjoint), unresolved deps, duplicate deps, completed-vs-pending
//! ready-set transitions, the empty graph, single nodes, and the determinism /
//! idempotency invariants the engine relies on.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use darkrun_core::dag::{Dag, DagEdge, DagNode, UnresolvedDep};
use darkrun_core::domain::{Status, Unit, UnitFrontmatter};
use darkrun_core::error::CoreError;

// ---------------------------------------------------------------------------
// Builders / helpers
// ---------------------------------------------------------------------------

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

fn pending(slug: &str, deps: &[&str]) -> Unit {
    unit(slug, Status::Pending, deps)
}

fn done(slug: &str, deps: &[&str]) -> Unit {
    unit(slug, Status::Completed, deps)
}

/// Position of a slug in a topo order. Panics if absent (a test assertion).
fn pos(order: &[String], slug: &str) -> usize {
    order
        .iter()
        .position(|x| x == slug)
        .unwrap_or_else(|| panic!("{slug} missing from order {order:?}"))
}

/// Assert that `before` precedes `after` in the topo order.
fn assert_before(order: &[String], before: &str, after: &str) {
    assert!(
        pos(order, before) < pos(order, after),
        "expected {before} before {after} in {order:?}"
    );
}

/// Verify a topo order is a valid linearisation: every edge `from -> to` has
/// `from` before `to`, and the order contains exactly the node set once.
fn assert_valid_topo(dag: &Dag, order: &[String]) {
    assert_eq!(order.len(), dag.nodes.len(), "order covers every node");
    let set: BTreeSet<&String> = order.iter().collect();
    assert_eq!(set.len(), order.len(), "no duplicates in order");
    for n in &dag.nodes {
        assert!(set.contains(&n.id), "node {} present", n.id);
    }
    for e in &dag.edges {
        assert_before(order, &e.from, &e.to);
    }
}

/// Reconstruct the wave assignment per node id from `waves()`.
fn wave_of(waves: &BTreeMap<usize, Vec<String>>, slug: &str) -> usize {
    for (w, group) in waves {
        if group.iter().any(|g| g == slug) {
            return *w;
        }
    }
    panic!("{slug} not in any wave: {waves:?}");
}

/// A small deterministic LCG so "random" graphs are reproducible without deps.
struct Lcg(u64);
impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1))
    }
    fn next(&mut self) -> u64 {
        // Numerical Recipes LCG constants.
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

/// Build a deterministic DAG of `n` nodes where node `i` may depend only on
/// strictly-earlier nodes (guarantees acyclicity). Deterministic per seed.
fn random_dag(n: usize, seed: u64) -> Vec<Unit> {
    let mut rng = Lcg::new(seed);
    let mut units = Vec::with_capacity(n);
    for i in 0..n {
        let slug = format!("n{i:04}");
        let mut deps = Vec::new();
        if i > 0 {
            // up to 3 back-edges into earlier nodes
            let k = rng.below(4);
            for _ in 0..k {
                let target = rng.below(i as u64) as usize;
                deps.push(format!("n{target:04}"));
            }
        }
        let dep_refs: Vec<&str> = deps.iter().map(|s| s.as_str()).collect();
        units.push(pending(&slug, &dep_refs));
    }
    units
}

// ===========================================================================
// EMPTY / SINGLE
// ===========================================================================

#[test]
fn empty_build_has_no_nodes() {
    let d = Dag::build(&[]);
    assert!(d.nodes.is_empty());
}

#[test]
fn empty_build_has_no_edges() {
    assert!(Dag::build(&[]).edges.is_empty());
}

#[test]
fn empty_build_has_no_unresolved() {
    assert!(Dag::build(&[]).unresolved.is_empty());
}

#[test]
fn empty_build_has_empty_adjacency() {
    assert!(Dag::build(&[]).adjacency.is_empty());
}

#[test]
fn empty_topo_is_empty() {
    assert!(Dag::build(&[]).topological_sort().expect("topo").is_empty());
}

#[test]
fn empty_waves_is_empty() {
    assert!(Dag::build(&[]).waves().expect("waves").is_empty());
}

#[test]
fn empty_ready_is_empty() {
    let d = Dag::build(&[]);
    assert!(d.ready_units(&[]).is_empty());
}

#[test]
fn single_node_topo() {
    let u = vec![pending("solo", &[])];
    assert_eq!(Dag::build(&u).topological_sort().expect("topo"), vec!["solo"]);
}

#[test]
fn single_node_one_wave() {
    let u = vec![pending("solo", &[])];
    let w = Dag::build(&u).waves().expect("waves");
    assert_eq!(w.len(), 1);
    assert_eq!(w[&0], vec!["solo".to_string()]);
}

#[test]
fn single_node_adjacency_present_and_empty() {
    let u = vec![pending("solo", &[])];
    let d = Dag::build(&u);
    assert!(d.adjacency.contains_key("solo"));
    assert!(d.adjacency["solo"].is_empty());
}

#[test]
fn single_node_no_edges() {
    let u = vec![pending("solo", &[])];
    assert!(Dag::build(&u).edges.is_empty());
}

#[test]
fn single_pending_no_deps_is_ready() {
    let u = vec![pending("solo", &[])];
    let d = Dag::build(&u);
    let r: Vec<&str> = d.ready_units(&u).iter().map(|x| x.slug.as_str()).collect();
    assert_eq!(r, vec!["solo"]);
}

#[test]
fn single_completed_node_not_ready() {
    let u = vec![done("solo", &[])];
    assert!(Dag::build(&u).ready_units(&u).is_empty());
}

#[test]
fn single_node_has_one_node_with_status() {
    let u = vec![unit("solo", Status::Active, &[])];
    let d = Dag::build(&u);
    assert_eq!(d.nodes.len(), 1);
    assert_eq!(d.nodes[0].id, "solo");
    assert_eq!(d.nodes[0].status, Status::Active);
}

// ===========================================================================
// LINEAR CHAINS (parametric over many lengths)
// ===========================================================================

fn linear_chain(n: usize) -> Vec<Unit> {
    let mut units = Vec::with_capacity(n);
    for i in 0..n {
        let slug = format!("c{i:03}");
        let deps: Vec<String> = if i == 0 {
            vec![]
        } else {
            vec![format!("c{:03}", i - 1)]
        };
        let dep_refs: Vec<&str> = deps.iter().map(|s| s.as_str()).collect();
        units.push(pending(&slug, &dep_refs));
    }
    units
}

macro_rules! linear_len_tests {
    ($($name:ident => $n:expr),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                let n = $n;
                let units = linear_chain(n);
                let dag = Dag::build(&units);
                let order = dag.topological_sort().expect("topo");
                assert_valid_topo(&dag, &order);
                // strict total order: c000, c001, ...
                for i in 0..n {
                    assert_eq!(order[i], format!("c{i:03}"));
                }
                // each node sits in its own wave equal to its index
                let waves = dag.waves().expect("waves");
                assert_eq!(waves.len(), n);
                for i in 0..n {
                    assert_eq!(wave_of(&waves, &format!("c{i:03}")), i);
                    assert_eq!(waves[&i], vec![format!("c{i:03}")]);
                }
                // edge count == n-1
                assert_eq!(dag.edges.len(), n.saturating_sub(1));
            }
        )*
    };
}

linear_len_tests! {
    linear_len_2 => 2,
    linear_len_3 => 3,
    linear_len_4 => 4,
    linear_len_5 => 5,
    linear_len_6 => 6,
    linear_len_7 => 7,
    linear_len_8 => 8,
    linear_len_10 => 10,
    linear_len_12 => 12,
    linear_len_16 => 16,
    linear_len_20 => 20,
    linear_len_25 => 25,
    linear_len_32 => 32,
    linear_len_40 => 40,
    linear_len_50 => 50,
    linear_len_64 => 64,
    linear_len_75 => 75,
    linear_len_100 => 100,
    linear_len_128 => 128,
    linear_len_150 => 150,
    linear_len_200 => 200,
}

#[test]
fn linear_chain_only_head_ready_when_nothing_done() {
    let units = linear_chain(6);
    let d = Dag::build(&units);
    let r: Vec<&str> = d.ready_units(&units).iter().map(|x| x.slug.as_str()).collect();
    // c000 has no deps; all others have a pending predecessor.
    assert_eq!(r, vec!["c000"]);
}

#[test]
fn linear_chain_advances_one_at_a_time() {
    // Complete the chain head-to-tail; exactly one new node becomes ready each step.
    let n = 8;
    for completed_through in 0..n {
        let mut units = linear_chain(n);
        for u in units.iter_mut().take(completed_through) {
            u.frontmatter.status = Status::Completed;
        }
        let d = Dag::build(&units);
        let r: Vec<&str> = d.ready_units(&units).iter().map(|x| x.slug.as_str()).collect();
        if completed_through < n {
            assert_eq!(r, vec![format!("c{completed_through:03}").as_str()]);
        } else {
            assert!(r.is_empty());
        }
    }
}

#[test]
fn linear_chain_topo_is_stable_across_runs() {
    let units = linear_chain(30);
    let d = Dag::build(&units);
    let first = d.topological_sort().expect("topo");
    for _ in 0..10 {
        assert_eq!(d.topological_sort().expect("topo"), first);
    }
}

// ===========================================================================
// DIAMOND topologies
// ===========================================================================

#[test]
fn classic_diamond_order_constraints() {
    let units = vec![
        pending("a", &[]),
        pending("b", &["a"]),
        pending("c", &["a"]),
        pending("d", &["b", "c"]),
    ];
    let d = Dag::build(&units);
    let o = d.topological_sort().expect("topo");
    assert_before(&o, "a", "b");
    assert_before(&o, "a", "c");
    assert_before(&o, "b", "d");
    assert_before(&o, "c", "d");
}

#[test]
fn classic_diamond_waves() {
    let units = vec![
        pending("a", &[]),
        pending("b", &["a"]),
        pending("c", &["a"]),
        pending("d", &["b", "c"]),
    ];
    let w = Dag::build(&units).waves().expect("waves");
    assert_eq!(w[&0], vec!["a".to_string()]);
    assert_eq!(w[&1], vec!["b".to_string(), "c".to_string()]);
    assert_eq!(w[&2], vec!["d".to_string()]);
}

#[test]
fn classic_diamond_deterministic_topo() {
    // With sorted seeding, this graph's topo order is fully determined.
    let units = vec![
        pending("a", &[]),
        pending("b", &["a"]),
        pending("c", &["a"]),
        pending("d", &["b", "c"]),
    ];
    let d = Dag::build(&units);
    assert_eq!(d.topological_sort().expect("topo"), vec!["a", "b", "c", "d"]);
}

#[test]
fn diamond_ready_set_when_apex_completed() {
    let units = vec![
        done("a", &[]),
        pending("b", &["a"]),
        pending("c", &["a"]),
        pending("d", &["b", "c"]),
    ];
    let d = Dag::build(&units);
    let r: Vec<&str> = d.ready_units(&units).iter().map(|x| x.slug.as_str()).collect();
    assert_eq!(r, vec!["b", "c"]);
}

#[test]
fn diamond_sink_ready_only_when_both_arms_done() {
    let units = vec![
        done("a", &[]),
        done("b", &["a"]),
        pending("c", &["a"]),
        pending("d", &["b", "c"]),
    ];
    let d = Dag::build(&units);
    let r: Vec<&str> = d.ready_units(&units).iter().map(|x| x.slug.as_str()).collect();
    // c is ready (a done), d blocked (c pending).
    assert_eq!(r, vec!["c"]);
}

#[test]
fn diamond_sink_ready_when_all_arms_done() {
    let units = vec![
        done("a", &[]),
        done("b", &["a"]),
        done("c", &["a"]),
        pending("d", &["b", "c"]),
    ];
    let d = Dag::build(&units);
    let r: Vec<&str> = d.ready_units(&units).iter().map(|x| x.slug.as_str()).collect();
    assert_eq!(r, vec!["d"]);
}

#[test]
fn nested_double_diamond_waves() {
    // a -> {b,c} -> d -> {e,f} -> g
    let units = vec![
        pending("a", &[]),
        pending("b", &["a"]),
        pending("c", &["a"]),
        pending("d", &["b", "c"]),
        pending("e", &["d"]),
        pending("f", &["d"]),
        pending("g", &["e", "f"]),
    ];
    let w = Dag::build(&units).waves().expect("waves");
    assert_eq!(w[&0], vec!["a".to_string()]);
    assert_eq!(w[&1], vec!["b".to_string(), "c".to_string()]);
    assert_eq!(w[&2], vec!["d".to_string()]);
    assert_eq!(w[&3], vec!["e".to_string(), "f".to_string()]);
    assert_eq!(w[&4], vec!["g".to_string()]);
}

#[test]
fn skewed_diamond_longest_path_wins_wave() {
    // a -> b -> c -> sink  and  a -> sink (short arm).
    // sink's wave is governed by the longest path (through c), not the short one.
    let units = vec![
        pending("a", &[]),
        pending("b", &["a"]),
        pending("c", &["b"]),
        pending("sink", &["a", "c"]),
    ];
    let w = Dag::build(&units).waves().expect("waves");
    assert_eq!(wave_of(&w, "a"), 0);
    assert_eq!(wave_of(&w, "b"), 1);
    assert_eq!(wave_of(&w, "c"), 2);
    assert_eq!(wave_of(&w, "sink"), 3);
}

// parametric diamonds with varying arm widths

fn fan_diamond(arms: usize) -> Vec<Unit> {
    let mut units = vec![pending("root", &[])];
    let mut arm_names = Vec::new();
    for i in 0..arms {
        let slug = format!("arm{i:02}");
        units.push(pending(&slug, &["root"]));
        arm_names.push(slug);
    }
    let refs: Vec<&str> = arm_names.iter().map(|s| s.as_str()).collect();
    units.push(pending("sink", &refs));
    units
}

macro_rules! fan_diamond_tests {
    ($($name:ident => $arms:expr),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                let arms = $arms;
                let units = fan_diamond(arms);
                let dag = Dag::build(&units);
                let order = dag.topological_sort().expect("topo");
                assert_valid_topo(&dag, &order);
                let w = dag.waves().expect("waves");
                assert_eq!(w[&0], vec!["root".to_string()]);
                assert_eq!(w[&1].len(), arms);
                assert_eq!(w[&2], vec!["sink".to_string()]);
                // sink depends on every arm: edge count = arms (root->arm) + arms (arm->sink)
                assert_eq!(dag.edges.len(), arms * 2);
            }
        )*
    };
}

fan_diamond_tests! {
    fan_diamond_2 => 2,
    fan_diamond_3 => 3,
    fan_diamond_4 => 4,
    fan_diamond_5 => 5,
    fan_diamond_6 => 6,
    fan_diamond_8 => 8,
    fan_diamond_10 => 10,
    fan_diamond_16 => 16,
    fan_diamond_24 => 24,
    fan_diamond_32 => 32,
    fan_diamond_50 => 50,
    fan_diamond_64 => 64,
    fan_diamond_100 => 100,
}

// ===========================================================================
// WIDE graphs (many independent roots)
// ===========================================================================

fn wide_roots(n: usize) -> Vec<Unit> {
    (0..n).map(|i| pending(&format!("r{i:04}"), &[])).collect()
}

macro_rules! wide_tests {
    ($($name:ident => $n:expr),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                let n = $n;
                let units = wide_roots(n);
                let dag = Dag::build(&units);
                let order = dag.topological_sort().expect("topo");
                assert_eq!(order.len(), n);
                // all roots are independent -> single wave 0
                let w = dag.waves().expect("waves");
                assert_eq!(w.len(), 1);
                assert_eq!(w[&0].len(), n);
                // topo is the sorted slug list (deterministic seeding)
                let mut expected: Vec<String> = (0..n).map(|i| format!("r{i:04}")).collect();
                expected.sort();
                assert_eq!(order, expected);
                assert!(dag.edges.is_empty());
                // all pending, no deps -> all ready
                assert_eq!(dag.ready_units(&units).len(), n);
            }
        )*
    };
}

wide_tests! {
    wide_2 => 2,
    wide_3 => 3,
    wide_4 => 4,
    wide_5 => 5,
    wide_8 => 8,
    wide_10 => 10,
    wide_16 => 16,
    wide_20 => 20,
    wide_32 => 32,
    wide_50 => 50,
    wide_64 => 64,
    wide_100 => 100,
    wide_128 => 128,
    wide_200 => 200,
    wide_256 => 256,
    wide_500 => 500,
}

// ===========================================================================
// DEEP + WIDE grid (layers of fixed width)
// ===========================================================================

/// `depth` layers, each `width` wide; every node depends on every node in the
/// previous layer. Produces depth waves of width each.
fn layered_grid(depth: usize, width: usize) -> Vec<Unit> {
    let mut units = Vec::new();
    for layer in 0..depth {
        for col in 0..width {
            let slug = format!("l{layer:02}c{col:02}");
            let deps: Vec<String> = if layer == 0 {
                vec![]
            } else {
                (0..width).map(|c| format!("l{:02}c{c:02}", layer - 1)).collect()
            };
            let refs: Vec<&str> = deps.iter().map(|s| s.as_str()).collect();
            units.push(pending(&slug, &refs));
        }
    }
    units
}

macro_rules! grid_tests {
    ($($name:ident => ($d:expr, $w:expr)),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                let (depth, width) = ($d, $w);
                let units = layered_grid(depth, width);
                let dag = Dag::build(&units);
                let order = dag.topological_sort().expect("topo");
                assert_valid_topo(&dag, &order);
                let waves = dag.waves().expect("waves");
                assert_eq!(waves.len(), depth);
                for layer in 0..depth {
                    assert_eq!(waves[&layer].len(), width, "layer {layer} width");
                    for col in 0..width {
                        assert_eq!(wave_of(&waves, &format!("l{layer:02}c{col:02}")), layer);
                    }
                }
            }
        )*
    };
}

grid_tests! {
    grid_2x2 => (2, 2),
    grid_2x3 => (2, 3),
    grid_3x2 => (3, 2),
    grid_3x3 => (3, 3),
    grid_3x4 => (3, 4),
    grid_4x3 => (4, 3),
    grid_4x4 => (4, 4),
    grid_5x2 => (5, 2),
    grid_5x5 => (5, 5),
    grid_6x3 => (6, 3),
    grid_8x2 => (8, 2),
    grid_8x4 => (8, 4),
    grid_10x3 => (10, 3),
    grid_12x2 => (12, 2),
    grid_16x4 => (16, 4),
    grid_20x2 => (20, 2),
}

// ===========================================================================
// CYCLES
// ===========================================================================

#[test]
fn self_loop_resolves_edge_not_unresolved() {
    let units = vec![pending("a", &["a"])];
    let d = Dag::build(&units);
    assert!(d.unresolved.is_empty());
    assert_eq!(d.edges.len(), 1);
    assert_eq!(d.edges[0], DagEdge { from: "a".into(), to: "a".into() });
}

#[test]
fn self_loop_is_a_cycle() {
    let units = vec![pending("a", &["a"])];
    assert!(matches!(
        Dag::build(&units).topological_sort().unwrap_err(),
        CoreError::CyclicDependency(_)
    ));
}

#[test]
fn self_loop_waves_errors() {
    let units = vec![pending("a", &["a"])];
    assert!(matches!(
        Dag::build(&units).waves().unwrap_err(),
        CoreError::CyclicDependency(_)
    ));
}

#[test]
fn self_loop_message_names_node() {
    let units = vec![pending("lonely", &["lonely"])];
    match Dag::build(&units).topological_sort().unwrap_err() {
        CoreError::CyclicDependency(s) => assert!(s.contains("lonely")),
        other => panic!("expected cycle, got {other:?}"),
    }
}

#[test]
fn two_node_cycle_detected() {
    let units = vec![pending("a", &["b"]), pending("b", &["a"])];
    assert!(matches!(
        Dag::build(&units).topological_sort().unwrap_err(),
        CoreError::CyclicDependency(_)
    ));
}

#[test]
fn two_node_cycle_names_both() {
    let units = vec![pending("a", &["b"]), pending("b", &["a"])];
    match Dag::build(&units).topological_sort().unwrap_err() {
        CoreError::CyclicDependency(s) => {
            assert!(s.contains("a"));
            assert!(s.contains("b"));
        }
        other => panic!("expected cycle, got {other:?}"),
    }
}

#[test]
fn two_node_cycle_waves_errors_too() {
    let units = vec![pending("a", &["b"]), pending("b", &["a"])];
    assert!(matches!(
        Dag::build(&units).waves().unwrap_err(),
        CoreError::CyclicDependency(_)
    ));
}

// long cycles of varying length

fn ring(n: usize) -> Vec<Unit> {
    // node i depends on node i-1 (mod n) -> one big cycle.
    (0..n)
        .map(|i| {
            let dep = format!("k{:03}", (i + n - 1) % n);
            unit(&format!("k{i:03}"), Status::Pending, &[dep.as_str()])
        })
        .collect()
}

macro_rules! ring_cycle_tests {
    ($($name:ident => $n:expr),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                let n = $n;
                let units = ring(n);
                let dag = Dag::build(&units);
                assert!(dag.unresolved.is_empty(), "ring deps all resolve");
                match dag.topological_sort().unwrap_err() {
                    CoreError::CyclicDependency(s) => {
                        // every node is part of the single cycle
                        for i in 0..n {
                            assert!(s.contains(&format!("k{i:03}")), "node k{i:03} reported");
                        }
                    }
                    other => panic!("expected cycle, got {other:?}"),
                }
                assert!(matches!(dag.waves().unwrap_err(), CoreError::CyclicDependency(_)));
            }
        )*
    };
}

ring_cycle_tests! {
    ring_3 => 3,
    ring_4 => 4,
    ring_5 => 5,
    ring_6 => 6,
    ring_7 => 7,
    ring_8 => 8,
    ring_10 => 10,
    ring_12 => 12,
    ring_16 => 16,
    ring_20 => 20,
    ring_32 => 32,
    ring_50 => 50,
    ring_64 => 64,
    ring_100 => 100,
}

#[test]
fn cycle_with_clean_prefix_only_reports_cycle_nodes() {
    // root sorts cleanly; a<->b form a 2-cycle downstream.
    let units = vec![
        pending("root", &[]),
        pending("a", &["root", "b"]),
        pending("b", &["a"]),
    ];
    match Dag::build(&units).topological_sort().unwrap_err() {
        CoreError::CyclicDependency(s) => {
            assert!(!s.contains("root"), "root is acyclic: {s}");
            assert!(s.contains("a") && s.contains("b"));
        }
        other => panic!("expected cycle, got {other:?}"),
    }
}

#[test]
fn cycle_with_clean_tail_only_reports_cycle_nodes() {
    // a<->b cycle, tail depends on nothing.
    let units = vec![
        pending("tail", &[]),
        pending("a", &["b"]),
        pending("b", &["a"]),
    ];
    match Dag::build(&units).topological_sort().unwrap_err() {
        CoreError::CyclicDependency(s) => {
            assert!(!s.contains("tail"));
            assert!(s.contains("a") && s.contains("b"));
        }
        other => panic!("expected cycle, got {other:?}"),
    }
}

#[test]
fn two_disjoint_cycles_both_reported() {
    let units = vec![
        pending("a", &["b"]),
        pending("b", &["a"]),
        pending("x", &["y"]),
        pending("y", &["x"]),
    ];
    match Dag::build(&units).topological_sort().unwrap_err() {
        CoreError::CyclicDependency(s) => {
            for slug in ["a", "b", "x", "y"] {
                assert!(s.contains(slug), "{slug} in {s}");
            }
        }
        other => panic!("expected cycle, got {other:?}"),
    }
}

#[test]
fn dag_with_acyclic_island_plus_cycle_keeps_island_out_of_report() {
    // Island: p -> q (clean). Cycle: c1 <-> c2.
    let units = vec![
        pending("p", &[]),
        pending("q", &["p"]),
        pending("c1", &["c2"]),
        pending("c2", &["c1"]),
    ];
    match Dag::build(&units).topological_sort().unwrap_err() {
        CoreError::CyclicDependency(s) => {
            assert!(!s.contains("p"));
            assert!(!s.contains('q'));
            assert!(s.contains("c1") && s.contains("c2"));
        }
        other => panic!("expected cycle, got {other:?}"),
    }
}

#[test]
fn self_loop_amid_clean_nodes_reports_only_self() {
    let units = vec![
        pending("clean1", &[]),
        pending("clean2", &["clean1"]),
        pending("loop", &["loop"]),
    ];
    match Dag::build(&units).topological_sort().unwrap_err() {
        CoreError::CyclicDependency(s) => {
            assert!(s.contains("loop"));
            assert!(!s.contains("clean1"));
            assert!(!s.contains("clean2"));
        }
        other => panic!("expected cycle, got {other:?}"),
    }
}

#[test]
fn long_chain_into_cycle_reports_only_cycle() {
    // c0 -> c1 -> c2 -> c3 clean, then a<->b cycle hanging off c3.
    let units = vec![
        pending("c0", &[]),
        pending("c1", &["c0"]),
        pending("c2", &["c1"]),
        pending("c3", &["c2"]),
        pending("a", &["c3", "b"]),
        pending("b", &["a"]),
    ];
    match Dag::build(&units).topological_sort().unwrap_err() {
        CoreError::CyclicDependency(s) => {
            for clean in ["c0", "c1", "c2", "c3"] {
                assert!(!s.contains(clean), "{clean} should be sorted");
            }
            assert!(s.contains("a") && s.contains("b"));
        }
        other => panic!("expected cycle, got {other:?}"),
    }
}

// ===========================================================================
// UNRESOLVED dependencies
// ===========================================================================

#[test]
fn single_unresolved_dep_collected() {
    let units = vec![pending("a", &["ghost"])];
    let d = Dag::build(&units);
    assert_eq!(d.unresolved.len(), 1);
    assert_eq!(
        d.unresolved[0],
        UnresolvedDep { unit: "a".into(), dep: "ghost".into() }
    );
}

#[test]
fn unresolved_produces_no_edge() {
    let units = vec![pending("a", &["ghost"])];
    assert!(Dag::build(&units).edges.is_empty());
}

#[test]
fn unresolved_dep_still_sorts() {
    let units = vec![pending("a", &["ghost"])];
    assert_eq!(Dag::build(&units).topological_sort().expect("topo"), vec!["a"]);
}

#[test]
fn multiple_unresolved_deps_all_collected() {
    let units = vec![pending("a", &["g1", "g2", "g3"])];
    let d = Dag::build(&units);
    assert_eq!(d.unresolved.len(), 3);
    let deps: BTreeSet<&str> = d.unresolved.iter().map(|u| u.dep.as_str()).collect();
    assert_eq!(deps, BTreeSet::from(["g1", "g2", "g3"]));
}

#[test]
fn unresolved_records_declaring_unit() {
    let units = vec![pending("real", &[]), pending("b", &["nope"])];
    let d = Dag::build(&units);
    assert_eq!(d.unresolved.len(), 1);
    assert_eq!(d.unresolved[0].unit, "b");
    assert_eq!(d.unresolved[0].dep, "nope");
}

#[test]
fn mixed_resolved_and_unresolved() {
    let units = vec![
        done("a", &[]),
        pending("b", &["a", "ghost"]),
    ];
    let d = Dag::build(&units);
    // one real edge a->b, one unresolved ghost.
    assert_eq!(d.edges.len(), 1);
    assert_eq!(d.edges[0], DagEdge { from: "a".into(), to: "b".into() });
    assert_eq!(d.unresolved.len(), 1);
    assert_eq!(d.unresolved[0].dep, "ghost");
}

#[test]
fn unresolved_dep_keeps_unit_unready() {
    // b depends on a ghost: never ready, since ghost is not Completed.
    let units = vec![pending("b", &["ghost"])];
    assert!(Dag::build(&units).ready_units(&units).is_empty());
}

#[test]
fn unresolved_preserves_order_of_declaration() {
    let units = vec![pending("a", &["z", "y", "x"])];
    let d = Dag::build(&units);
    let order: Vec<&str> = d.unresolved.iter().map(|u| u.dep.as_str()).collect();
    assert_eq!(order, vec!["z", "y", "x"]);
}

#[test]
fn all_deps_unresolved_yields_empty_graph_topo() {
    let units = vec![
        pending("a", &["ghost1"]),
        pending("b", &["ghost2"]),
    ];
    let d = Dag::build(&units);
    assert!(d.edges.is_empty());
    assert_eq!(d.unresolved.len(), 2);
    let order = d.topological_sort().expect("topo");
    assert_eq!(order, vec!["a", "b"]);
    let w = d.waves().expect("waves");
    assert_eq!(w.len(), 1); // both in wave 0 (no real edges)
    assert_eq!(w[&0], vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn unresolved_does_not_pollute_adjacency() {
    let units = vec![pending("a", &["ghost"])];
    let d = Dag::build(&units);
    // adjacency has only declared nodes, not ghosts.
    assert!(d.adjacency.contains_key("a"));
    assert!(!d.adjacency.contains_key("ghost"));
}

// ===========================================================================
// DUPLICATE dependencies
// ===========================================================================

#[test]
fn duplicate_dep_yields_two_edges() {
    let units = vec![done("a", &[]), pending("b", &["a", "a"])];
    let d = Dag::build(&units);
    let count = d.edges.iter().filter(|e| e.from == "a" && e.to == "b").count();
    assert_eq!(count, 2);
}

#[test]
fn duplicate_dep_still_topo_sorts() {
    let units = vec![done("a", &[]), pending("b", &["a", "a"])];
    assert_eq!(
        Dag::build(&units).topological_sort().expect("topo"),
        vec!["a", "b"]
    );
}

#[test]
fn triple_duplicate_dep_yields_three_edges() {
    let units = vec![done("a", &[]), pending("b", &["a", "a", "a"])];
    let d = Dag::build(&units);
    assert_eq!(d.edges.iter().filter(|e| e.to == "b").count(), 3);
    // adjacency from a contains b three times (no dedupe)
    assert_eq!(d.adjacency["a"].iter().filter(|x| *x == "b").count(), 3);
}

#[test]
fn duplicate_dep_waves_unaffected() {
    let units = vec![pending("a", &[]), pending("b", &["a", "a", "a"])];
    let w = Dag::build(&units).waves().expect("waves");
    assert_eq!(wave_of(&w, "a"), 0);
    assert_eq!(wave_of(&w, "b"), 1);
}

#[test]
fn duplicate_dep_ready_set_counts_once_logically() {
    // ready_units uses .all() over deps, so duplicates don't change readiness.
    let units = vec![done("a", &[]), pending("b", &["a", "a"])];
    let d = Dag::build(&units);
    let r: Vec<&str> = d.ready_units(&units).iter().map(|x| x.slug.as_str()).collect();
    assert_eq!(r, vec!["b"]);
}

#[test]
fn duplicate_unresolved_dep_recorded_twice() {
    let units = vec![pending("a", &["ghost", "ghost"])];
    let d = Dag::build(&units);
    assert_eq!(d.unresolved.iter().filter(|u| u.dep == "ghost").count(), 2);
}

#[test]
fn mixed_duplicate_real_and_ghost() {
    let units = vec![
        done("a", &[]),
        pending("b", &["a", "a", "ghost", "ghost"]),
    ];
    let d = Dag::build(&units);
    assert_eq!(d.edges.iter().filter(|e| e.to == "b").count(), 2);
    assert_eq!(d.unresolved.len(), 2);
}

// ===========================================================================
// READY-SET transitions (completed vs pending)
// ===========================================================================

#[test]
fn ready_requires_pending_status() {
    for s in [Status::Active, Status::InProgress, Status::Completed, Status::Blocked] {
        let units = vec![unit("solo", s, &[])];
        assert!(
            Dag::build(&units).ready_units(&units).is_empty(),
            "status {s:?} should not be ready"
        );
    }
}

#[test]
fn ready_pending_no_deps_is_ready() {
    let units = vec![pending("solo", &[])];
    assert_eq!(Dag::build(&units).ready_units(&units).len(), 1);
}

#[test]
fn ready_dep_must_be_completed_not_merely_active() {
    for dep_status in [Status::Pending, Status::Active, Status::InProgress, Status::Blocked] {
        let units = vec![
            unit("a", dep_status, &[]),
            pending("b", &["a"]),
        ];
        let d = Dag::build(&units);
        let ready: Vec<&str> = d.ready_units(&units).iter().map(|x| x.slug.as_str()).collect();
        // b is never ready while its dep is not Completed.
        assert!(
            !ready.contains(&"b"),
            "dep status {dep_status:?} must block b's readiness"
        );
    }
}

#[test]
fn ready_dep_completed_unblocks() {
    let units = vec![done("a", &[]), pending("b", &["a"])];
    let d = Dag::build(&units);
    let r: Vec<&str> = d.ready_units(&units).iter().map(|x| x.slug.as_str()).collect();
    assert_eq!(r, vec!["b"]);
}

#[test]
fn ready_all_deps_must_be_completed() {
    // b needs a AND c. Only ready when both done.
    let mk = |a: Status, c: Status| {
        vec![
            unit("a", a, &[]),
            unit("c", c, &[]),
            pending("b", &["a", "c"]),
        ]
    };
    let b_ready = |units: &[Unit]| {
        Dag::build(units)
            .ready_units(units)
            .iter()
            .any(|u| u.slug == "b")
    };
    // Both done -> b ready.
    let u1 = mk(Status::Completed, Status::Completed);
    assert!(b_ready(&u1));
    // One done -> b not ready (the still-pending dep blocks it).
    let u2 = mk(Status::Completed, Status::Pending);
    assert!(!b_ready(&u2));
    let u3 = mk(Status::Pending, Status::Completed);
    assert!(!b_ready(&u3));
}

#[test]
fn ready_preserves_input_iteration_order() {
    // ready_units filters over the slice in order.
    let units = vec![
        pending("z", &[]),
        pending("a", &[]),
        pending("m", &[]),
    ];
    let d = Dag::build(&units);
    let r: Vec<&str> = d.ready_units(&units).iter().map(|x| x.slug.as_str()).collect();
    assert_eq!(r, vec!["z", "a", "m"]);
}

#[test]
fn ready_set_walks_a_chain_to_completion() {
    // As we mark units done in topo order, exactly the next unit becomes ready.
    let base = || vec![
        unit("a", Status::Pending, &[]),
        unit("b", Status::Pending, &["a"]),
        unit("c", Status::Pending, &["b"]),
        unit("d", Status::Pending, &["c"]),
    ];

    // step 0
    let u = base();
    assert_eq!(ready_slugs(&u), vec!["a"]);

    // step 1: a done
    let mut u = base();
    u[0].frontmatter.status = Status::Completed;
    assert_eq!(ready_slugs(&u), vec!["b"]);

    // step 2: a,b done
    let mut u = base();
    u[0].frontmatter.status = Status::Completed;
    u[1].frontmatter.status = Status::Completed;
    assert_eq!(ready_slugs(&u), vec!["c"]);

    // step 3: a,b,c done
    let mut u = base();
    for i in 0..3 {
        u[i].frontmatter.status = Status::Completed;
    }
    assert_eq!(ready_slugs(&u), vec!["d"]);

    // step 4: all done -> nothing ready
    let mut u = base();
    for x in u.iter_mut() {
        x.frontmatter.status = Status::Completed;
    }
    assert!(ready_slugs(&u).is_empty());
}

fn ready_slugs(units: &[Unit]) -> Vec<String> {
    Dag::build(units)
        .ready_units(units)
        .iter()
        .map(|u| u.slug.clone())
        .collect()
}

#[test]
fn ready_diamond_apex_then_arms_then_sink() {
    // Walk a diamond's readiness frontier.
    let base = || vec![
        unit("a", Status::Pending, &[]),
        unit("b", Status::Pending, &["a"]),
        unit("c", Status::Pending, &["a"]),
        unit("d", Status::Pending, &["b", "c"]),
    ];
    // nothing done -> a
    assert_eq!(ready_slugs(&base()), vec!["a"]);
    // a done -> b,c
    let mut u = base();
    u[0].frontmatter.status = Status::Completed;
    assert_eq!(ready_slugs(&u), vec!["b", "c"]);
    // a,b,c done -> d
    let mut u = base();
    for i in 0..3 {
        u[i].frontmatter.status = Status::Completed;
    }
    assert_eq!(ready_slugs(&u), vec!["d"]);
}

#[test]
fn ready_blocked_status_unit_never_surfaces() {
    let units = vec![
        done("a", &[]),
        unit("b", Status::Blocked, &["a"]),
    ];
    assert!(Dag::build(&units).ready_units(&units).is_empty());
}

#[test]
fn ready_empty_deps_list_treated_as_ready() {
    // explicit empty deps == no deps.
    let units = vec![pending("a", &[])];
    assert_eq!(ready_slugs(&units), vec!["a"]);
}

#[test]
fn ready_independent_pendings_all_surface() {
    let units = vec![
        pending("a", &[]),
        pending("b", &[]),
        pending("c", &[]),
    ];
    assert_eq!(ready_slugs(&units), vec!["a", "b", "c"]);
}

#[test]
fn ready_partial_completion_among_independent() {
    let units = vec![
        done("a", &[]),
        pending("b", &[]),
        done("c", &[]),
        pending("d", &[]),
    ];
    assert_eq!(ready_slugs(&units), vec!["b", "d"]);
}

// ===========================================================================
// ADJACENCY / NODE structure invariants
// ===========================================================================

#[test]
fn every_node_appears_in_adjacency() {
    let units = vec![
        pending("a", &[]),
        pending("b", &["a"]),
        pending("c", &["b"]),
    ];
    let d = Dag::build(&units);
    for n in &d.nodes {
        assert!(d.adjacency.contains_key(&n.id), "{} in adjacency", n.id);
    }
}

#[test]
fn adjacency_points_from_dep_to_dependent() {
    let units = vec![pending("a", &[]), pending("b", &["a"])];
    let d = Dag::build(&units);
    assert_eq!(d.adjacency["a"], vec!["b".to_string()]);
    assert!(d.adjacency["b"].is_empty());
}

#[test]
fn leaf_node_has_empty_adjacency() {
    let units = vec![pending("a", &[]), pending("b", &["a"])];
    let d = Dag::build(&units);
    assert!(d.adjacency["b"].is_empty());
}

#[test]
fn nodes_preserve_input_order() {
    let units = vec![
        pending("zebra", &[]),
        pending("alpha", &[]),
        pending("mango", &[]),
    ];
    let d = Dag::build(&units);
    let ids: Vec<&str> = d.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["zebra", "alpha", "mango"]);
}

#[test]
fn nodes_carry_status() {
    let units = vec![
        unit("a", Status::Completed, &[]),
        unit("b", Status::Blocked, &[]),
        unit("c", Status::InProgress, &[]),
    ];
    let d = Dag::build(&units);
    assert_eq!(d.nodes[0], DagNode { id: "a".into(), status: Status::Completed });
    assert_eq!(d.nodes[1], DagNode { id: "b".into(), status: Status::Blocked });
    assert_eq!(d.nodes[2], DagNode { id: "c".into(), status: Status::InProgress });
}

#[test]
fn edges_record_from_and_to_directionally() {
    let units = vec![pending("up", &[]), pending("down", &["up"])];
    let d = Dag::build(&units);
    assert_eq!(d.edges.len(), 1);
    assert_eq!(d.edges[0].from, "up");
    assert_eq!(d.edges[0].to, "down");
}

#[test]
fn fan_out_adjacency_lists_all_dependents() {
    let units = vec![
        pending("root", &[]),
        pending("a", &["root"]),
        pending("b", &["root"]),
        pending("c", &["root"]),
    ];
    let d = Dag::build(&units);
    let mut got = d.adjacency["root"].clone();
    got.sort();
    assert_eq!(got, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
}

#[test]
fn fan_in_node_has_multiple_incoming_edges() {
    let units = vec![
        pending("a", &[]),
        pending("b", &[]),
        pending("c", &[]),
        pending("sink", &["a", "b", "c"]),
    ];
    let d = Dag::build(&units);
    let incoming = d.edges.iter().filter(|e| e.to == "sink").count();
    assert_eq!(incoming, 3);
}

// ===========================================================================
// DETERMINISM / IDEMPOTENCY
// ===========================================================================

#[test]
fn build_is_idempotent_for_nodes_and_edges() {
    let units = random_dag(40, 7);
    let d1 = Dag::build(&units);
    let d2 = Dag::build(&units);
    assert_eq!(d1.nodes, d2.nodes);
    assert_eq!(d1.edges, d2.edges);
    assert_eq!(d1.adjacency, d2.adjacency);
    assert_eq!(d1.unresolved, d2.unresolved);
}

#[test]
fn topo_is_deterministic_for_random_dag() {
    let units = random_dag(60, 11);
    let d = Dag::build(&units);
    let first = d.topological_sort().expect("topo");
    for _ in 0..8 {
        assert_eq!(d.topological_sort().expect("topo"), first);
    }
}

#[test]
fn waves_is_deterministic_for_random_dag() {
    let units = random_dag(60, 13);
    let d = Dag::build(&units);
    let first = d.waves().expect("waves");
    for _ in 0..8 {
        assert_eq!(d.waves().expect("waves"), first);
    }
}

#[test]
fn topo_independent_roots_come_out_sorted() {
    let units = vec![
        pending("zebra", &[]),
        pending("alpha", &[]),
        pending("mango", &[]),
        pending("delta", &[]),
    ];
    assert_eq!(
        Dag::build(&units).topological_sort().expect("topo"),
        vec!["alpha", "delta", "mango", "zebra"]
    );
}

#[test]
fn topo_order_independent_of_input_ordering() {
    // Same logical graph, units supplied in different orders -> same topo
    // (because seeding is sorted and adjacency is a BTreeMap).
    let a = vec![
        pending("a", &[]),
        pending("b", &["a"]),
        pending("c", &["a"]),
        pending("d", &["b", "c"]),
    ];
    let b = vec![
        pending("d", &["b", "c"]),
        pending("c", &["a"]),
        pending("b", &["a"]),
        pending("a", &[]),
    ];
    assert_eq!(
        Dag::build(&a).topological_sort().expect("topo"),
        Dag::build(&b).topological_sort().expect("topo")
    );
}

#[test]
fn waves_independent_of_input_ordering() {
    let a = vec![
        pending("a", &[]),
        pending("b", &["a"]),
        pending("c", &["b"]),
    ];
    let b = vec![
        pending("c", &["b"]),
        pending("a", &[]),
        pending("b", &["a"]),
    ];
    assert_eq!(
        Dag::build(&a).waves().expect("waves"),
        Dag::build(&b).waves().expect("waves")
    );
}

// ===========================================================================
// RANDOM-BUT-DETERMINISTIC DAGS — validity over many seeds/sizes
// ===========================================================================

macro_rules! random_validity_tests {
    ($($name:ident => ($n:expr, $seed:expr)),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                let units = random_dag($n, $seed);
                let dag = Dag::build(&units);
                // By construction node i only depends on earlier nodes -> acyclic.
                let order = dag.topological_sort().expect("acyclic by construction");
                assert_valid_topo(&dag, &order);

                // waves succeed and partition every node exactly once.
                let waves = dag.waves().expect("waves");
                let mut seen: HashSet<String> = HashSet::new();
                for group in waves.values() {
                    for slug in group {
                        assert!(seen.insert(slug.clone()), "{slug} in one wave only");
                    }
                }
                assert_eq!(seen.len(), $n);

                // Wave of a node strictly exceeds the wave of each of its real deps.
                for e in &dag.edges {
                    let wf = wave_of(&waves, &e.from);
                    let wt = wave_of(&waves, &e.to);
                    assert!(wt > wf, "edge {}->{} must climb a wave", e.from, e.to);
                }

                // No back-edges -> no unresolved (all deps point at real, earlier slugs).
                assert!(dag.unresolved.is_empty());
            }
        )*
    };
}

random_validity_tests! {
    random_n10_s1 => (10, 1),
    random_n10_s2 => (10, 2),
    random_n10_s3 => (10, 3),
    random_n20_s1 => (20, 1),
    random_n20_s2 => (20, 2),
    random_n20_s3 => (20, 3),
    random_n30_s5 => (30, 5),
    random_n30_s9 => (30, 9),
    random_n40_s7 => (40, 7),
    random_n40_s17 => (40, 17),
    random_n50_s4 => (50, 4),
    random_n50_s8 => (50, 8),
    random_n60_s2 => (60, 2),
    random_n75_s6 => (75, 6),
    random_n80_s11 => (80, 11),
    random_n100_s1 => (100, 1),
    random_n100_s2 => (100, 2),
    random_n100_s3 => (100, 3),
    random_n128_s13 => (128, 13),
    random_n150_s19 => (150, 19),
    random_n200_s23 => (200, 23),
    random_n256_s29 => (256, 29),
}

#[test]
fn random_dag_topo_respects_every_edge_many_seeds() {
    for seed in 0..40u64 {
        let units = random_dag(50, seed);
        let dag = Dag::build(&units);
        let order = dag.topological_sort().expect("acyclic");
        for e in &dag.edges {
            assert_before(&order, &e.from, &e.to);
        }
    }
}

#[test]
fn random_dag_ready_set_is_subset_of_pending_roots_when_none_done() {
    // With nothing completed, ready == pending units with all deps... but no
    // dep is completed, so ready == pending units that have zero deps.
    for seed in 0..20u64 {
        let units = random_dag(40, seed);
        let dag = Dag::build(&units);
        let ready: HashSet<String> = dag
            .ready_units(&units)
            .iter()
            .map(|u| u.slug.clone())
            .collect();
        for u in &units {
            let zero_dep = u.frontmatter.depends_on.is_empty();
            assert_eq!(
                ready.contains(&u.slug),
                zero_dep && u.frontmatter.status == Status::Pending,
                "readiness for {} (deps {:?})",
                u.slug,
                u.frontmatter.depends_on
            );
        }
    }
}

#[test]
fn random_dag_all_completed_means_nothing_ready() {
    for seed in 0..20u64 {
        let mut units = random_dag(40, seed);
        for u in units.iter_mut() {
            u.frontmatter.status = Status::Completed;
        }
        assert!(Dag::build(&units).ready_units(&units).is_empty());
    }
}

#[test]
fn random_dag_completing_a_topo_prefix_keeps_ready_within_pending() {
    // Complete the first half (in topo order) and verify every "ready" unit is
    // pending with all deps completed — the engine's core invariant.
    for seed in 0..15u64 {
        let units = random_dag(40, seed);
        let dag = Dag::build(&units);
        let order = dag.topological_sort().expect("topo");
        let prefix: HashSet<&String> = order.iter().take(order.len() / 2).collect();

        let mut mutated = units.clone();
        for u in mutated.iter_mut() {
            if prefix.contains(&u.slug) {
                u.frontmatter.status = Status::Completed;
            }
        }

        let dag2 = Dag::build(&mutated);
        let status: BTreeMap<String, Status> = mutated
            .iter()
            .map(|u| (u.slug.clone(), u.frontmatter.status))
            .collect();
        for ready in dag2.ready_units(&mutated) {
            assert_eq!(ready.frontmatter.status, Status::Pending);
            for dep in &ready.frontmatter.depends_on {
                assert_eq!(status.get(dep), Some(&Status::Completed));
            }
        }
    }
}

// ===========================================================================
// MULTI-COMPONENT / DISJOINT graphs
// ===========================================================================

#[test]
fn two_disjoint_chains_interleave_in_topo_by_sort() {
    // Chain1: a -> b. Chain2: x -> y. Independent components.
    let units = vec![
        pending("a", &[]),
        pending("b", &["a"]),
        pending("x", &[]),
        pending("y", &["x"]),
    ];
    let d = Dag::build(&units);
    let o = d.topological_sort().expect("topo");
    assert_before(&o, "a", "b");
    assert_before(&o, "x", "y");
    // Both roots a,x share wave 0; both leaves b,y share wave 1.
    let w = d.waves().expect("waves");
    assert_eq!(w[&0], vec!["a".to_string(), "x".to_string()]);
    assert_eq!(w[&1], vec!["b".to_string(), "y".to_string()]);
}

#[test]
fn three_disjoint_components_share_wave_structure() {
    let mut units = Vec::new();
    for c in ["p", "q", "r"] {
        units.push(pending(&format!("{c}0"), &[]));
        units.push(pending(&format!("{c}1"), &[&format!("{c}0")]));
        units.push(pending(&format!("{c}2"), &[&format!("{c}1")]));
    }
    let w = Dag::build(&units).waves().expect("waves");
    assert_eq!(w.len(), 3);
    assert_eq!(w[&0].len(), 3);
    assert_eq!(w[&1].len(), 3);
    assert_eq!(w[&2].len(), 3);
}

#[test]
fn disjoint_graph_one_component_completed_only_affects_its_own_ready() {
    let units = vec![
        done("a", &[]),
        pending("b", &["a"]),
        pending("x", &[]),
        pending("y", &["x"]),
    ];
    let d = Dag::build(&units);
    let r: BTreeSet<String> = d.ready_units(&units).iter().map(|u| u.slug.clone()).collect();
    // b ready (a done), x ready (root, pending), y blocked (x pending).
    assert_eq!(r, BTreeSet::from(["b".to_string(), "x".to_string()]));
}

// ===========================================================================
// TREE topologies
// ===========================================================================

/// Complete binary tree where children depend on the parent. `levels` deep.
fn binary_tree(levels: usize) -> Vec<Unit> {
    let count = (1usize << levels) - 1; // nodes 0..count
    let mut units = Vec::with_capacity(count);
    for i in 0..count {
        let slug = format!("t{i:03}");
        let deps: Vec<String> = if i == 0 {
            vec![]
        } else {
            vec![format!("t{:03}", (i - 1) / 2)]
        };
        let refs: Vec<&str> = deps.iter().map(|s| s.as_str()).collect();
        units.push(pending(&slug, &refs));
    }
    units
}

macro_rules! binary_tree_tests {
    ($($name:ident => $levels:expr),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                let levels = $levels;
                let units = binary_tree(levels);
                let dag = Dag::build(&units);
                let order = dag.topological_sort().expect("topo");
                assert_valid_topo(&dag, &order);
                // Tree of L levels has L waves; wave L has 2^L nodes.
                let waves = dag.waves().expect("waves");
                assert_eq!(waves.len(), levels);
                for lvl in 0..levels {
                    assert_eq!(waves[&lvl].len(), 1 << lvl, "wave {lvl} width");
                }
            }
        )*
    };
}

binary_tree_tests! {
    binary_tree_1 => 1,
    binary_tree_2 => 2,
    binary_tree_3 => 3,
    binary_tree_4 => 4,
    binary_tree_5 => 5,
    binary_tree_6 => 6,
    binary_tree_7 => 7,
    binary_tree_8 => 8,
}

#[test]
fn tree_root_only_ready_then_children() {
    let units = binary_tree(3);
    // root only
    assert_eq!(ready_slugs(&units), vec!["t000"]);
    // root done -> its two children ready
    let mut u = units.clone();
    u[0].frontmatter.status = Status::Completed;
    let r = ready_slugs(&u);
    assert_eq!(r, vec!["t001", "t002"]);
}

// ===========================================================================
// WAVE invariants (general)
// ===========================================================================

#[test]
fn wave_zero_holds_exactly_the_roots() {
    let units = vec![
        pending("r1", &[]),
        pending("r2", &[]),
        pending("a", &["r1"]),
        pending("b", &["r2"]),
    ];
    let w = Dag::build(&units).waves().expect("waves");
    assert_eq!(w[&0], vec!["r1".to_string(), "r2".to_string()]);
}

#[test]
fn waves_are_contiguous_from_zero() {
    let units = linear_chain(10);
    let w = Dag::build(&units).waves().expect("waves");
    let keys: Vec<usize> = w.keys().copied().collect();
    assert_eq!(keys, (0..10).collect::<Vec<_>>());
}

#[test]
fn every_node_lands_in_exactly_one_wave() {
    let units = random_dag(50, 99);
    let dag = Dag::build(&units);
    let w = dag.waves().expect("waves");
    let total: usize = w.values().map(|g| g.len()).sum();
    assert_eq!(total, dag.nodes.len());
    let unique: HashSet<&String> = w.values().flatten().collect();
    assert_eq!(unique.len(), dag.nodes.len());
}

#[test]
fn wave_groups_are_internally_sorted() {
    let units = vec![
        pending("root", &[]),
        pending("zed", &["root"]),
        pending("abe", &["root"]),
        pending("mid", &["root"]),
    ];
    let w = Dag::build(&units).waves().expect("waves");
    assert_eq!(w[&1], vec!["abe".to_string(), "mid".to_string(), "zed".to_string()]);
}

#[test]
fn node_wave_exceeds_all_dependency_waves() {
    let units = random_dag(60, 31);
    let dag = Dag::build(&units);
    let w = dag.waves().expect("waves");
    for e in &dag.edges {
        assert!(wave_of(&w, &e.to) > wave_of(&w, &e.from));
    }
}

#[test]
fn single_long_chain_has_n_waves() {
    for n in [2, 5, 13, 27, 64] {
        let units = linear_chain(n);
        assert_eq!(Dag::build(&units).waves().expect("waves").len(), n);
    }
}

// ===========================================================================
// MIXED / REALISTIC scenarios
// ===========================================================================

#[test]
fn complex_graph_full_invariants() {
    // A hand-built graph with multiple roots, a diamond, a fan-out, and a leaf.
    let units = vec![
        pending("spec", &[]),
        pending("design", &["spec"]),
        pending("build_a", &["design"]),
        pending("build_b", &["design"]),
        pending("integrate", &["build_a", "build_b"]),
        pending("docs", &["spec"]),
        pending("ship", &["integrate", "docs"]),
    ];
    let d = Dag::build(&units);
    assert!(d.unresolved.is_empty());
    let o = d.topological_sort().expect("topo");
    assert_valid_topo(&d, &o);
    let w = d.waves().expect("waves");
    assert_eq!(wave_of(&w, "spec"), 0);
    assert_eq!(wave_of(&w, "design"), 1);
    assert_eq!(wave_of(&w, "docs"), 1);
    assert_eq!(wave_of(&w, "build_a"), 2);
    assert_eq!(wave_of(&w, "build_b"), 2);
    assert_eq!(wave_of(&w, "integrate"), 3);
    assert_eq!(wave_of(&w, "ship"), 4);
}

#[test]
fn complex_graph_ready_frontier_advances() {
    let base = || vec![
        unit("spec", Status::Pending, &[]),
        unit("design", Status::Pending, &["spec"]),
        unit("docs", Status::Pending, &["spec"]),
        unit("build", Status::Pending, &["design"]),
        unit("ship", Status::Pending, &["build", "docs"]),
    ];
    // start: spec
    assert_eq!(ready_slugs(&base()), vec!["spec"]);
    // spec done -> design, docs
    let mut u = base();
    u[0].frontmatter.status = Status::Completed;
    assert_eq!(ready_slugs(&u), vec!["design", "docs"]);
    // spec, design, docs done -> build
    let mut u = base();
    for i in [0, 1, 2] {
        u[i].frontmatter.status = Status::Completed;
    }
    assert_eq!(ready_slugs(&u), vec!["build"]);
    // all but ship done -> ship
    let mut u = base();
    for i in 0..4 {
        u[i].frontmatter.status = Status::Completed;
    }
    assert_eq!(ready_slugs(&u), vec!["ship"]);
}

#[test]
fn graph_with_mixed_statuses_topo_ignores_status() {
    // Status doesn't affect topo ordering, only ready_units.
    let units = vec![
        done("a", &[]),
        unit("b", Status::Active, &["a"]),
        unit("c", Status::Blocked, &["b"]),
    ];
    assert_eq!(
        Dag::build(&units).topological_sort().expect("topo"),
        vec!["a", "b", "c"]
    );
}

#[test]
fn graph_with_mixed_statuses_waves_ignore_status() {
    let units = vec![
        done("a", &[]),
        unit("b", Status::Active, &["a"]),
        unit("c", Status::Blocked, &["b"]),
    ];
    let w = Dag::build(&units).waves().expect("waves");
    assert_eq!(wave_of(&w, "a"), 0);
    assert_eq!(wave_of(&w, "b"), 1);
    assert_eq!(wave_of(&w, "c"), 2);
}

// ===========================================================================
// EDGE / STRESS — node count, edge count bookkeeping
// ===========================================================================

#[test]
fn node_count_equals_unit_count() {
    for n in [1, 5, 33, 128] {
        let units = wide_roots(n);
        assert_eq!(Dag::build(&units).nodes.len(), n);
    }
}

#[test]
fn dense_dag_edge_count_is_exact() {
    // Each node i (1..n) depends on all earlier nodes 0..i -> C(n,2) edges.
    let n = 12;
    let mut units = Vec::new();
    for i in 0..n {
        let slug = format!("d{i:02}");
        let deps: Vec<String> = (0..i).map(|j| format!("d{j:02}")).collect();
        let refs: Vec<&str> = deps.iter().map(|s| s.as_str()).collect();
        units.push(pending(&slug, &refs));
    }
    let d = Dag::build(&units);
    assert_eq!(d.edges.len(), n * (n - 1) / 2);
    // It's a total order: every node in its own wave.
    let w = d.waves().expect("waves");
    assert_eq!(w.len(), n);
}

#[test]
fn dense_dag_topo_is_the_total_order() {
    let n = 10;
    let mut units = Vec::new();
    for i in 0..n {
        let slug = format!("d{i:02}");
        let deps: Vec<String> = (0..i).map(|j| format!("d{j:02}")).collect();
        let refs: Vec<&str> = deps.iter().map(|s| s.as_str()).collect();
        units.push(pending(&slug, &refs));
    }
    let order = Dag::build(&units).topological_sort().expect("topo");
    let expected: Vec<String> = (0..n).map(|i| format!("d{i:02}")).collect();
    assert_eq!(order, expected);
}

#[test]
fn large_wide_graph_single_wave_many_nodes() {
    let units = wide_roots(1000);
    let d = Dag::build(&units);
    let w = d.waves().expect("waves");
    assert_eq!(w.len(), 1);
    assert_eq!(w[&0].len(), 1000);
    assert_eq!(d.ready_units(&units).len(), 1000);
}

#[test]
fn large_deep_chain_sorts_in_order() {
    let units = linear_chain(500);
    let order = Dag::build(&units).topological_sort().expect("topo");
    assert_eq!(order.len(), 500);
    for i in 0..500 {
        assert_eq!(order[i], format!("c{i:03}"));
    }
}

// ===========================================================================
// DEFAULT / clone semantics on the Dag struct
// ===========================================================================

#[test]
fn default_dag_is_empty() {
    let d = Dag::default();
    assert!(d.nodes.is_empty());
    assert!(d.edges.is_empty());
    assert!(d.adjacency.is_empty());
    assert!(d.unresolved.is_empty());
    assert!(d.topological_sort().expect("topo").is_empty());
    assert!(d.waves().expect("waves").is_empty());
}

#[test]
fn cloned_dag_sorts_identically() {
    let units = random_dag(30, 77);
    let d = Dag::build(&units);
    let clone = d.clone();
    assert_eq!(
        d.topological_sort().expect("topo"),
        clone.topological_sort().expect("topo")
    );
    assert_eq!(d.waves().expect("waves"), clone.waves().expect("waves"));
}

// ===========================================================================
// SUB-STRUCT equality / debug
// ===========================================================================

#[test]
fn dag_node_equality() {
    let a = DagNode { id: "x".into(), status: Status::Pending };
    let b = DagNode { id: "x".into(), status: Status::Pending };
    let c = DagNode { id: "x".into(), status: Status::Active };
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn dag_edge_equality_is_directional() {
    let fwd = DagEdge { from: "a".into(), to: "b".into() };
    let bwd = DagEdge { from: "b".into(), to: "a".into() };
    assert_ne!(fwd, bwd);
    assert_eq!(fwd, DagEdge { from: "a".into(), to: "b".into() });
}

#[test]
fn unresolved_dep_equality() {
    let a = UnresolvedDep { unit: "u".into(), dep: "d".into() };
    assert_eq!(a, UnresolvedDep { unit: "u".into(), dep: "d".into() });
    assert_ne!(a, UnresolvedDep { unit: "u".into(), dep: "e".into() });
}

#[test]
fn dag_node_debug_contains_id() {
    let n = DagNode { id: "frame".into(), status: Status::Pending };
    assert!(format!("{n:?}").contains("frame"));
}

// ===========================================================================
// READY-SET edge with self/cyclic deps (build still succeeds)
// ===========================================================================

#[test]
fn ready_units_on_self_loop_pending_is_never_ready() {
    // a depends on itself; a is pending; a is not Completed so not ready.
    let units = vec![pending("a", &["a"])];
    assert!(Dag::build(&units).ready_units(&units).is_empty());
}

#[test]
fn ready_units_does_not_require_acyclicity() {
    // ready_units never calls topo; it works even on a cyclic graph.
    let units = vec![
        unit("a", Status::Pending, &["b"]),
        unit("b", Status::Completed, &["a"]),
    ];
    let d = Dag::build(&units);
    // a is pending and its dep b is Completed -> a is ready, despite the cycle.
    let r: Vec<&str> = d.ready_units(&units).iter().map(|x| x.slug.as_str()).collect();
    assert_eq!(r, vec!["a"]);
    // and the graph is genuinely cyclic.
    assert!(d.topological_sort().is_err());
}

// ===========================================================================
// PRE-COMPLETED graphs: ready set carved by completion frontier
// ===========================================================================

#[test]
fn fully_completed_chain_has_empty_ready() {
    let mut units = linear_chain(8);
    for u in units.iter_mut() {
        u.frontmatter.status = Status::Completed;
    }
    assert!(Dag::build(&units).ready_units(&units).is_empty());
}

#[test]
fn middle_completed_unblocks_next_only() {
    // chain c0..c4; c0,c1 completed -> c2 ready, c3,c4 blocked.
    let mut units = linear_chain(5);
    units[0].frontmatter.status = Status::Completed;
    units[1].frontmatter.status = Status::Completed;
    assert_eq!(ready_slugs(&units), vec!["c002"]);
}

#[test]
fn skipping_completion_does_not_unblock_downstream() {
    // Completing a non-prefix (c2 done but c1 not) leaves c2's child still blocked
    // on c2? No — c3 depends on c2 (done) so c3 becomes ready; but c1 (pending)
    // makes c2... already done. Validate: c0 ready (pending root), c3 ready.
    let mut units = linear_chain(5);
    units[2].frontmatter.status = Status::Completed;
    let r = ready_slugs(&units);
    // c0 has no deps and pending -> ready. c3 depends on c2 (done) -> ready.
    // c1 depends on c0 (pending) -> blocked. c4 depends on c3 (pending) -> blocked.
    assert_eq!(r, vec!["c000", "c003"]);
}

// ===========================================================================
// EMPTY deps vs absent deps equivalence
// ===========================================================================

#[test]
fn explicit_empty_deps_equivalent_to_default() {
    let with_empty = vec![pending("a", &[])];
    let with_default = vec![Unit {
        slug: "a".into(),
        frontmatter: UnitFrontmatter { status: Status::Pending, ..Default::default() },
        title: "a".into(),
        body: String::new(),
    }];
    let d1 = Dag::build(&with_empty);
    let d2 = Dag::build(&with_default);
    assert_eq!(d1.edges, d2.edges);
    assert_eq!(d1.nodes, d2.nodes);
    assert_eq!(
        d1.topological_sort().expect("t1"),
        d2.topological_sort().expect("t2")
    );
}

// ===========================================================================
// ORDERING tie-breaks resolved by slug, not by input position
// ===========================================================================

#[test]
fn topo_ties_broken_by_slug_lexicographically() {
    // root unblocks three children at once; they must emerge slug-sorted.
    let units = vec![
        pending("root", &[]),
        pending("ccc", &["root"]),
        pending("aaa", &["root"]),
        pending("bbb", &["root"]),
    ];
    let order = Dag::build(&units).topological_sort().expect("topo");
    assert_eq!(order, vec!["root", "aaa", "bbb", "ccc"]);
}

#[test]
fn topo_ties_at_multiple_levels() {
    // Two roots; each spawns two children. All ties slug-ordered.
    let units = vec![
        pending("rb", &[]),
        pending("ra", &[]),
        pending("a2", &["ra"]),
        pending("a1", &["ra"]),
        pending("b2", &["rb"]),
        pending("b1", &["rb"]),
    ];
    let order = Dag::build(&units).topological_sort().expect("topo");
    // Roots first (ra, rb), then their children interleaved by slug as they unblock.
    // Kahn pops the smallest available each step.
    assert_before(&order, "ra", "a1");
    assert_before(&order, "ra", "a2");
    assert_before(&order, "rb", "b1");
    assert_before(&order, "rb", "b2");
    assert_before(&order, "a1", "a2");
    assert_before(&order, "b1", "b2");
}

// ===========================================================================
// UNRESOLVED + cycle interplay
// ===========================================================================

#[test]
fn unresolved_dep_on_cyclic_node_still_cycles() {
    // a<->b cycle; a also declares a ghost dep (unresolved, no edge).
    let units = vec![
        pending("a", &["b", "ghost"]),
        pending("b", &["a"]),
    ];
    let d = Dag::build(&units);
    assert_eq!(d.unresolved.len(), 1);
    assert!(matches!(
        d.topological_sort().unwrap_err(),
        CoreError::CyclicDependency(_)
    ));
}

// ===========================================================================
// Repeated build/sort over the SAME units yields stable structure hashes
// ===========================================================================

#[test]
fn rebuild_yields_identical_edge_vector() {
    let units = random_dag(40, 101);
    let e1 = Dag::build(&units).edges;
    let e2 = Dag::build(&units).edges;
    assert_eq!(e1, e2);
}

#[test]
fn rebuild_yields_identical_unresolved_vector() {
    let units = vec![
        pending("a", &["g1", "g2"]),
        pending("b", &["g3"]),
    ];
    let u1 = Dag::build(&units).unresolved;
    let u2 = Dag::build(&units).unresolved;
    assert_eq!(u1, u2);
}
