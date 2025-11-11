//! # Hyperdep
//!
//! A `no_std` compatible hypergraph library for representing many-to-many relationships.
//!
//! Hypergraphs extend traditional graphs by allowing edges to connect multiple nodes
//! simultaneously, making them ideal for dependency resolution and complex relationship modeling.
//!
//! ## Features
//!
//! - `petgraph`: Enables conversion to petgraph's bipartite representation
//!
//! ## Example
//!
//! ```
//! use hyperdep::HyperGraph;
//!
//! let mut graph = HyperGraph::new();
//! graph.insert("depends_on", ["package_a", "package_b"]);
//!
//! for node in graph.nodes_of(&"depends_on") {
//!     println!("Package depends on: {}", node);
//! }
//! ```

#![no_std]
extern crate alloc;

use alloc::collections::btree_map::Entry;
use alloc::collections::{BTreeMap, BTreeSet};
use core::fmt;

/// A node in the hypergraph that can be connected by edges.
pub trait Node: Ord + Clone + fmt::Debug {}
impl<T: Ord + Clone + fmt::Debug> Node for T {}

/// An edge in the hypergraph that can connect multiple nodes.
pub trait Edge: Ord + Clone + fmt::Debug {}
impl<T: Ord + Clone + fmt::Debug> Edge for T {}

/// A hypergraph data structure maintaining bidirectional mappings between nodes and edges.
///
/// This implementation uses BTreeMap and BTreeSet internally to ensure deterministic
/// iteration order and logarithmic-time operations.
#[derive(Debug, Clone)]
pub struct HyperGraph<N, E> {
    /// Maps each node to the set of edges connected to it
    node_to_edges: BTreeMap<N, BTreeSet<E>>,
    /// Maps each edge to the set of nodes it connects
    edge_to_nodes: BTreeMap<E, BTreeSet<N>>,
}

impl<N: Node, E: Edge> Default for HyperGraph<N, E> {
    fn default() -> Self {
        Self {
            node_to_edges: BTreeMap::new(),
            edge_to_nodes: BTreeMap::new(),
        }
    }
}

impl<N: Node, E: Edge> HyperGraph<N, E> {
    #[inline]
    /// Creates a new empty hypergraph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a new edge connecting the given nodes.
    ///
    /// Returns `true` if the edge was newly inserted, `false` if it already existed
    /// (in which case the node connections are updated).
    ///
    /// # Arguments
    ///
    /// * `edge` - The edge to insert
    /// * `nodes` - Iterator of nodes that this edge should connect
    pub fn insert(&mut self, edge: E, nodes: impl IntoIterator<Item = N>) -> bool {
        let mut node_set = BTreeSet::new();
        for n in nodes {
            self.node_to_edges
                .entry(n.clone())
                .or_default()
                .insert(edge.clone());
            node_set.insert(n);
        }
        self.edge_to_nodes.insert(edge.clone(), node_set).is_none()
    }

    /// Removes an edge and returns the nodes it was connecting.
    ///
    /// Returns `None` if the edge didn't exist.
    pub fn remove_edge(&mut self, edge: &E) -> Option<BTreeSet<N>> {
        let nodes = self.edge_to_nodes.remove(edge)?;
        for n in &nodes {
            if let Entry::Occupied(mut ent) = self.node_to_edges.entry(n.clone()) {
                ent.get_mut().remove(edge);
                if ent.get().is_empty() {
                    ent.remove();
                }
            }
        }
        Some(nodes)
    }

    #[inline]
    /// Returns an iterator over all nodes connected by the given edge.
    pub fn nodes_of(&self, edge: &E) -> impl Iterator<Item = &N> {
        self.edge_to_nodes.get(edge).into_iter().flatten()
    }

    #[inline]
    /// Returns an iterator over all edges connected to the given node.
    pub fn edges_of(&self, node: &N) -> impl Iterator<Item = &E> {
        self.node_to_edges.get(node).into_iter().flatten()
    }

    /// Converts this hypergraph into a bipartite graph representation using petgraph.
    ///
    /// In the bipartite representation:
    /// - Left side: Original nodes
    /// - Right side: Original edges
    /// - Edges: Connections between original nodes and edges
    ///
    /// Requires the `petgraph` feature.
    #[cfg(feature = "petgraph")]
    pub fn into_bipartite(self) -> petgraph::Graph<BipartiteNode<N, E>, ()>
    where
        N: Ord + Clone,
        E: Ord + Clone,
    {
        use petgraph::Graph;

        let mut g = Graph::new();
        let mut node_idx: BTreeMap<N, _> = BTreeMap::new();
        let mut edge_idx: BTreeMap<E, _> = BTreeMap::new();

        // Assign indices in sorted order (deterministic!)
        for (n, _) in &self.node_to_edges {
            let idx = g.add_node(BipartiteNode::Node(n.clone()));
            node_idx.insert(n.clone(), idx);
        }
        for (e, _) in &self.edge_to_nodes {
            let idx = g.add_node(BipartiteNode::Edge(e.clone()));
            edge_idx.insert(e.clone(), idx);
        }

        for (e, nodes) in self.edge_to_nodes {
            let e_idx = edge_idx[&e];
            for n in nodes {
                let n_idx = node_idx[&n];
                g.add_edge(n_idx, e_idx, ());
            }
        }
        g
    }
}

/// Represents a node in the bipartite graph conversion.
///
/// Used when converting a hypergraph to petgraph's bipartite representation.
#[cfg(feature = "petgraph")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum BipartiteNode<N, E> {
    /// An original node from the hypergraph
    Node(N),
    /// An original edge from the hypergraph
    Edge(E),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let mut hg = HyperGraph::<&str, &str>::new();
        hg.insert("e1", ["a", "b"]);
        assert_eq!(hg.nodes_of(&"e1").count(), 2);
    }

    #[test]
    #[cfg(feature = "petgraph")]
    fn bipartite_btreemap() {
        let mut hg = HyperGraph::<&str, &str>::new();
        hg.insert("e1", ["a", "b"]);

        let g = hg.into_bipartite();

        // 2 package nodes ("a", "b") + 1 hyperedge node ("e1") = 3
        assert_eq!(g.node_count(), 3);
        // Each package connected to the hyperedge → 2 edges
        assert_eq!(g.edge_count(), 2);
    }
}
