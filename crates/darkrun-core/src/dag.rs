//! Unit dependency graph: build, topological ordering, waves, ready-set.
//!
//! Pure graph logic — no rendering; visualization lives downstream in the
//! desktop app.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::domain::{Status, Unit};
use crate::error::{CoreError, Result};

/// A node in the unit dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagNode {
    /// The unit slug.
    pub id: String,
    /// The unit's lifecycle status.
    pub status: Status,
}

/// A directed edge `from -> to` (a dependency arrow: `to` depends on `from`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagEdge {
    /// The dependency (upstream) node.
    pub from: String,
    /// The dependent (downstream) node.
    pub to: String,
}

/// A reference to a `depends_on` entry that points at an unknown unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedDep {
    /// The unit that declared the dependency.
    pub unit: String,
    /// The unresolved dependency slug.
    pub dep: String,
}

/// The built dependency graph.
#[derive(Debug, Clone, Default)]
pub struct Dag {
    /// All nodes, in input order.
    pub nodes: Vec<DagNode>,
    /// All resolved edges.
    pub edges: Vec<DagEdge>,
    /// Adjacency list: dependency slug -> dependents.
    pub adjacency: BTreeMap<String, Vec<String>>,
    /// `depends_on` references that didn't resolve to a known unit.
    pub unresolved: Vec<UnresolvedDep>,
}

impl Dag {
    /// Build a DAG from parsed units using their `depends_on` fields.
    ///
    /// Unknown dependency slugs are collected in [`Dag::unresolved`] rather
    /// than producing edges.
    pub fn build(units: &[Unit]) -> Dag {
        let nodes: Vec<DagNode> = units
            .iter()
            .map(|u| DagNode {
                id: u.slug.clone(),
                status: u.frontmatter.status,
            })
            .collect();

        let slug_set: BTreeSet<&str> = units.iter().map(|u| u.slug.as_str()).collect();
        let mut adjacency: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for u in units {
            adjacency.entry(u.slug.clone()).or_default();
        }

        let mut edges = Vec::new();
        let mut unresolved = Vec::new();
        for u in units {
            for dep in &u.frontmatter.depends_on {
                if !slug_set.contains(dep.as_str()) {
                    unresolved.push(UnresolvedDep {
                        unit: u.slug.clone(),
                        dep: dep.clone(),
                    });
                    continue;
                }
                edges.push(DagEdge {
                    from: dep.clone(),
                    to: u.slug.clone(),
                });
                adjacency.entry(dep.clone()).or_default().push(u.slug.clone());
            }
        }

        Dag {
            nodes,
            edges,
            adjacency,
            unresolved,
        }
    }

    /// Topological sort via Kahn's algorithm, ordered deterministically.
    ///
    /// Returns node ids in dependency order. Errors with
    /// [`CoreError::CyclicDependency`] when a cycle is present.
    pub fn topological_sort(&self) -> Result<Vec<String>> {
        let mut in_degree: HashMap<&str, usize> =
            self.nodes.iter().map(|n| (n.id.as_str(), 0usize)).collect();
        for e in &self.edges {
            *in_degree.entry(e.to.as_str()).or_insert(0) += 1;
        }

        // Seed with zero-in-degree nodes, kept sorted for determinism.
        let mut queue: Vec<String> = in_degree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(id, _)| id.to_string())
            .collect();
        queue.sort();

        let mut sorted: Vec<String> = Vec::with_capacity(self.nodes.len());
        while !queue.is_empty() {
            let current = queue.remove(0);
            sorted.push(current.clone());
            if let Some(neighbors) = self.adjacency.get(&current) {
                for neighbor in neighbors {
                    let entry = in_degree.entry(neighbor.as_str()).or_insert(1);
                    *entry = entry.saturating_sub(1);
                    if *entry == 0 {
                        // Insert preserving sorted order.
                        let idx = queue.partition_point(|q| q.as_str() < neighbor.as_str());
                        queue.insert(idx, neighbor.clone());
                    }
                }
            }
        }

        if sorted.len() < self.nodes.len() {
            let cycle: Vec<String> = self
                .nodes
                .iter()
                .map(|n| n.id.clone())
                .filter(|id| !sorted.contains(id))
                .collect();
            return Err(CoreError::CyclicDependency(cycle.join(", ")));
        }
        Ok(sorted)
    }

    /// Group units into dependency waves.
    ///
    /// Wave 0 holds units with no dependencies; wave N holds units whose
    /// dependencies all sit in waves 0..N-1.
    pub fn waves(&self) -> Result<BTreeMap<usize, Vec<String>>> {
        let sorted = self.topological_sort()?;
        let mut node_wave: HashMap<String, usize> = HashMap::new();

        for node_id in &sorted {
            let deps: Vec<&str> = self
                .edges
                .iter()
                .filter(|e| &e.to == node_id)
                .map(|e| e.from.as_str())
                .collect();
            let wave = if deps.is_empty() {
                0
            } else {
                deps.iter()
                    .map(|d| node_wave.get(*d).copied().unwrap_or(0))
                    .max()
                    .unwrap_or(0)
                    + 1
            };
            node_wave.insert(node_id.clone(), wave);
        }

        let mut waves: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (node_id, wave) in node_wave {
            waves.entry(wave).or_default().push(node_id);
        }
        for group in waves.values_mut() {
            group.sort();
        }
        Ok(waves)
    }

    /// Compute the ready-set: pending units whose dependencies are all
    /// completed.
    pub fn ready_units<'a>(&self, units: &'a [Unit]) -> Vec<&'a Unit> {
        let status: HashMap<&str, Status> =
            self.nodes.iter().map(|n| (n.id.as_str(), n.status)).collect();

        units
            .iter()
            .filter(|u| {
                if u.frontmatter.status != Status::Pending {
                    return false;
                }
                u.frontmatter
                    .depends_on
                    .iter()
                    .all(|dep| status.get(dep.as_str()) == Some(&Status::Completed))
            })
            .collect()
    }
}
