# Eka: A New Foundation for the Software Supply Chain

> ⚠️ **Warning:** Eka is in early experimental stages. Features are unstable and subject to change.

This repository contains `eka`, a native frontend for the Atom Protocol, a new standard for decentralized software distribution. `eka` is a fundamentally new tool designed from the ground up to provide a more efficient, secure, and approachable development experience. It is the first step towards building a more resilient and transparent software supply chain, free from the single points of failure inherent in traditional, centralized package registries.

## Why Atom? The Future of Package Management

The Atom Format is more than just a new packaging standard; it's a fundamental rethinking of how we distribute and manage software. It addresses the inherent limitations of centralized package registries, paving the way for a more secure, efficient, and decentralized future.

### A Decentralized Replacement for Centralized Registries

Atom is a complete, decentralized replacement for traditional package registries like npm, PyPI, or crates.io. It solves the same core problem of dependency management without relying on a single point of failure. By leveraging the distributed nature of Git, Atom ensures that package availability is not tied to a single entity, eliminating a critical vulnerability in the software supply chain.

### End-to-End Security by Design

Atoms are not "copies" of source code like traditional packages. They are cryptographically verifiable, immutable slices of the source repository itself. This is achieved by creating a new, lightweight reference to the same underlying Git data objects that comprise the source code. This git-native approach means there is no possibility for drift between the source and the packaged code because no files are ever copied. While there are practical considerations (like the ongoing transition from SHA-1), the model provides a foundation for a fully end-to-end secure pipeline—from source code, to package, to the final build artifact when combined with tools like Nix. The implications for supply chain security are profound.

### Efficiency Without Compromise

Typically, developers face a trade-off between the convenience of centralized registries and the complexities of decentralized systems. Atom eliminates this trade-off. By intelligently creating unambiguous cryptographic IDs for atoms, it opens up a world of possibilities for highly efficient, decentralized build pipelines that can outperform even the largest centralized systems. We are building the foundation for a system that is not only more secure and resilient but also orders of magnitude more efficient than what is possible with today's centralized models.

This work is centered on four core components, which will eventually be unified into a single monorepo:

- **Eka:** A user-facing CLI that provides a reasonable, statically-determinable interface for managing dependencies and builds.
- **[Atom-Nix][atom-nix]:** A Nix module system for evaluating atoms.
- **Atom Format:** A verifiable, versioned, and git-native format for publishing source code, designed for decentralized distribution and end-to-end integrity.
- **Eos (Future):** A planned distributed, content-addressed build scheduler that will eventually power Eka's evaluation backend.

## The Nix Connection: A Foundation for Reproducible Builds

While the Atom Protocol's ambition is to be a universal, backend-agnostic standard, a verifiable build system is essential to translate its source code integrity into truly reproducible artifacts. `eka`'s inaugural implementation is therefore deeply integrated with the Nix ecosystem, which provides a powerful foundation for this vision.

The architecture is best understood as three distinct, decoupled layers:

1.  **Package Management:** This is `eka`'s primary domain. It manages `atom` dependencies and can even lock legacy Nix dependencies without requiring a local Nix installation. By creating a universal, decentralized source packaging layer, Atom simplifies the entire toolchain. The need for complex adapters like `*2nix` tools is reduced, as `eka` could natively understand and lock theoretical `crate` atoms just as easily as it does Nix atoms.
2.  **Evaluation:** This layer transforms dynamic, high-level code into a static, low-level build recipe (a Nix derivation). It resolves all variables and functions to produce a precise, unambiguous plan for the build.
3.  **Build:** This final layer takes the static build recipe from the evaluation stage and executes it in a sandboxed environment to produce the final, bit-for-bit identical artifact.

`eka` orchestrates this entire process, but the long-term vision is for the evaluation and build layers to be handled by a dedicated, distributed build scheduler, **Eos**. This clean separation of concerns is a core design principle, and it will eventually allow for a highly efficient, decentralized system where different stages of the pipeline can run on different machines. Nudging developers to think about these layers in a more disciplined fashion is a key goal, as the status quo of the Nix ecosystem is to have them tightly coupled.

This architecture ensures the Atom Protocol itself remains generic. Its ambition is to become a fully language-agnostic standard by its first stable release. In the future, other ecosystems could adopt it directly. For example, `cargo` could be taught to publish crates in the Atom format, with no knowledge of whether they would ultimately be consumed by a Nix build system. The format is a foundational layer, not the entire pipeline.

## Design Goals

- **Disciplined:** Eka focuses on its area of expertise: providing a fast, intuitive interface for managing dependencies with no external binary dependencies. It maintains a clean separation of concerns, delegating the heavy lifting of evaluation and building to a dedicated scheduler (Eos).
- **Fast:** The dependency management commands in `eka` are designed to be exceptionally fast, operating primarily on static metadata. Querying, resolving, and locking atoms are near-instantaneous operations.
- **Conceptually High-Level:** Developers care about packages, versions, security, and reproducibility, not the nitty-gritty of Nix derivations. Eka provides an interface that speaks to developers at their level of concern, while still providing a powerful gateway to the guarantees that Nix and the Atom Format provide.

## Core Concepts

### The Atom Format: Verifiable, Versioned Repository Slices

**Key Features:**

- **Repository Identity:** Every repository of atoms has a unique identity derived from its root commit.
- **Git-Native Publishing:** Atoms are published as new, lightweight references to pre-existing Git objects, with no copying of source files.
- **Temporal Conflict Resolution:** The system enforces that no two atoms in the same commit can share the same `atom.tag`, guaranteeing a unique cryptographic ID.
- **Efficient Version Discovery:** Atom versions are published to a queryable, decentralized index of Git references.

### The Atom URI: A User-Friendly Addressing Scheme

Atoms and other dependencies are addressed using a convenient URI format that supports aliases, scheme inference, and a special syntax for pinned dependencies. A critical design decision is that aliases are a **user interface-only concern**; they are fully expanded before being written to the manifest, ensuring that your project is always portable and reproducible.

### Manifest and Lockfile

Eka uses a standard `atom.toml` manifest and `atom.lock` lockfile to manage dependencies, similar to Cargo or npm.

- **`atom.toml`:** A declarative manifest where you define your project's dependencies, including both atoms and pinned legacy dependencies.
- **`atom.lock`:** A fully resolved lockfile that captures the exact versions and cryptographic hashes of all dependencies, ensuring that your builds are completely reproducible.

## Getting Started

### `eka publish`: Publishing Atoms

The `eka publish` command implements the in-source publishing strategy for atoms. Before publishing to a new remote for the first time, you must initialize it.

```bash
# Initialize the repository on a remote (only needs to be done once per remote).
eka publish --init --remote origin
```

Then, you can publish atoms from your current `HEAD` or a specified revision:

```bash
# Publish an atom from the current directory
eka publish .
```

<p align="center">
<a href="https://asciinema.org/a/uIcIOlELOVaPn15ICS2ZEH2CQ">
    <img src="https://asciinema.org/a/uIcIOlELOVaPn15ICS2ZEH2CQ.svg" alt="Publish Demo" height="256">
  </a>
</p>

### `eka add`: Adding and Locking Dependencies

The `eka add` command adds a new dependency to your `atom.toml` manifest and updates the `atom.lock` file.

```bash
# Add an atom dependency
eka add gh:owner/repo::my-atom@^1

# Add a pinned Git dependency
eka add gh:owner/repo^^some-branch
```

<p align="center">
  <a href="https://asciinema.org/a/qk7oNQIpDH0nsR0EsnRWsS7YQ">
    <img src="https://asciinema.org/a/qk7oNQIpDH0nsR0EsnRWsS7YQ.svg" alt="Add Demo" height="256">
  </a>
</p>

## Development

For a detailed breakdown of the development plan, please see the full [ROADMAP.md](./ROADMAP.md).

The architecture of Eka is guided by a series of Architectural Decision Records (ADRs). To learn more about the technical details, please refer to the [ADRs](./adrs). The atom's crate docs are also available at [docs.eka.rs][crate].

[eos]: https://github.com/ekala-project/eos-gateway
[atom-nix]: https://github.com/ekala-project/atom
[crate]: https://docs.eka.rs/atom
