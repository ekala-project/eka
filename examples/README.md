# Atom Examples

This directory contains practical examples of atom manifests for reference and learning. Each example demonstrates different aspects of dependency management, composition, and packaging in the Eka ecosystem.

## Getting Started

You can interact with these examples using the `eka` CLI tool. To do so:

1. Temporarily add an entry pointing to an example in the [ekala.toml](../ekala.toml) file in the repository root
2. Use `eka resolve` to generate lock files
3. Use `eka add` to add new dependencies

**Note**: Lock expression bootstrapping is still under development, so examples include basic Nix expressions to import lock expressions for evaluation.

## Examples

### Foo

This example demonstrates dependency management in Eka, showcasing:

- **Package Metadata**: Basic atom identification and versioning
- **Set Definitions**: Named collections of atom repositories with support for multiple mirrors, enabling decentralized atoms (repository identity determined by initialization commit)
- **Composition**: Using composer atoms that provide well-known APIs (like `atom-nix` for the atom module system)
- **Atom Dependencies**: Dependencies from other atom sets with version constraints
- **Direct Dependencies**: Various Nix dependency types including:
  - Git repositories with branch tracking
  - Tarball archives
  - Patch files
  - Build dependencies with unpacking

The atom re-exports its dependencies, making them available for interaction (e.g., in a Nix REPL).

**Files**:

- `atom.toml`: The manifest file with detailed comments explaining each section
- `atom.lock`: Auto-generated lock file (use `eka resolve` to update)
- `src/mod.nix`: Simple atom-nix module that exposes dependencies

## WIP

More examples demonstrating advanced features, are planned.
