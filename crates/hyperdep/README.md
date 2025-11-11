## README for hyperdep crate

# hyperdep

A `no_std` compatible hypergraph library for Rust, designed for dependency resolution and complex relationship modeling.

## What is a Hypergraph?

Unlike traditional graphs where edges connect exactly two nodes, hypergraphs allow edges to connect multiple nodes simultaneously. This makes them perfect for representing:

- Package dependencies (one package depends on many others)
- Feature flags and their requirements
- Complex many-to-many relationships
- Build system dependency graphs

## Features

- **No Standard Library**: Works in `no_std` environments with `alloc`
- **Deterministic**: Uses BTreeMap/BTreeSet for consistent iteration order
- **Optional petgraph Integration**: Convert to bipartite graphs for advanced algorithms
- **Type Safe**: Generic over node and edge types with trait bounds

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
hyperdep = "0.1"

# Optional: Enable petgraph integration
hyperdep = { version = "0.1", features = ["petgraph"] }
```

Basic usage:

```rust
use hyperdep::HyperGraph;

let mut graph = HyperGraph::new();

// Add a dependency relationship
graph.insert("build_script", ["serde", "tokio", "hyper"]);

// Query what a package depends on
for dep in graph.nodes_of(&"build_script") {
    println!("Depends on: {}", dep);
}

// Query what depends on a package
for consumer in graph.edges_of(&"serde") {
    println!("Used by: {}", consumer);
}
```

## Performance

- Insert/Remove: O(N log N) where N is number of nodes in the edge
- Query operations: O(log N) for map lookups + O(K) for set iteration
- Memory: O(V + E) where V is total nodes, E is total edges

## License

Licensed under the same terms as the Eka project.
