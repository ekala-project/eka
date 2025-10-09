# Eka: A New Foundation for the Software Supply Chain

> ⚠️ **Warning:** Eka is in early experimental stages. Features are unstable and subject to change.

`eka` is a command-line tool for managing software dependencies using the **Atom Protocol**, a new standard for decentralized software distribution. It is designed from the ground up to provide a more efficient, secure, and reproducible development experience.

This is the first step towards a more resilient and transparent software supply chain, free from the single points of failure inherent in traditional, centralized package registries.

## What is the Atom Protocol?

The [Atom Protocol](https://docs.eka.rs/atom/) is a rethinking of how we distribute and manage software. It addresses the inherent limitations of centralized package registries by focusing on three core principles:

- **Decentralized Distribution:** Instead of a central server like npm or PyPI, Atom uses Git repositories as the source of truth. It leverages the distributed nature of Git to ensure that package availability is not tied to a single entity, eliminating a critical vulnerability in the software supply chain.

- **Verifiable, Git-Native Packages:** Atoms are not "copies" of source code. They are cryptographically verifiable, immutable slices of a source repository. This is achieved by creating a new, lightweight reference to the same underlying Git objects that comprise the source code. There is no possibility for drift between the source and the packaged code because no files are ever copied.

- **Designed for Efficiency:** By creating unambiguous, content-addressed cryptographic IDs for every package, Atom enables highly efficient, decentralized build pipelines. This foundation allows for a system that is not only more secure and resilient but is also designed for high-performance, distributed build systems.

## How It Works: The Three Layers

Eka's architecture is best understood as three distinct, decoupled layers. This separation of concerns is a core design principle.

1.  **Package Management (Eka):** This is `eka`'s primary domain. It manages `atom` dependencies and generates a lockfile (`atom.lock`) that guarantees reproducible dependency resolution. It has no external binary dependencies and is designed to be exceptionally fast.

2.  **Evaluation:** This layer transforms the high-level dependency graph into a static, low-level build recipe (i.e. a Nix derivation). It resolves all inputs to produce a precise, unambiguous plan for the build.

3.  **Build:** This final layer takes the static build recipe and executes it in a sandboxed environment to produce a bit-for-bit identical artifact.

While `eka`'s inaugural implementation is deeply integrated with the **Nix ecosystem** for the Evaluation and Build layers, the Atom Protocol itself is language-agnostic. In the future, other build systems could be taught to consume the `atom.lock` format, making the protocol a universal standard.

## The Ekala Ecosystem

This work is centered on four core components, which will eventually be unified into a single monorepo:

- **Eka:** A user-facing CLI that provides a reasonable, statically-determinable interface for managing dependencies and builds.
- **[atom-nix]:** A Nix module system for evaluating atoms.
- **Atom Protocol:** A verifiable, versioned, and git-native format for publishing source code, designed for decentralized distribution and end-to-end integrity.
- **Eos (Future):** A planned distributed, content-addressed build scheduler that will eventually power Eka's evaluation backend.

## Core Concepts

- **Manifest (`atom.toml`):** A declarative file where you define your project's dependencies, including both atoms and pinned legacy sources (like a specific Git branch).
- **Lockfile (`atom.lock`):** A fully resolved lockfile that captures the exact versions and cryptographic hashes of all dependencies, ensuring that your builds are completely reproducible.
- **Atom URI:** A user-friendly addressing scheme for dependencies (e.g., `gh:owner/repo::my-atom@^1`). Aliases like `gh:` are a UI-only concern and are fully expanded in the lockfile to ensure portability.

## Core Commands

The following demos illustrate the two fundamental operations in `eka` with lay the foundation: publishing atoms and adding them as dependencies to a project.

### `eka publish`: Publishing Atoms

The `eka publish` command implements the in-source publishing strategy for atoms. It creates the necessary Git references in the source repository to make a new version of an atom available for consumption.

<p align="center">
  <a href="https://asciinema.org/a/uIcIOlELOVaPn15ICS2ZEH2CQ">
    <img src="https://asciinema.org/a/uIcIOlELOVaPn15ICS2ZEH2CQ.svg" alt="Publish Demo" height="256">
  </a>
</p>

### `eka add`: Adding and Locking Dependencies

The `eka add` command adds a new dependency to your `atom.toml` manifest and updates the `atom.lock` file with the resolved, cryptographically-verifiable version.

<p align="center">
  <a href="https://asciinema.org/a/qk7oNQIpDH0nsR0EsnRWsS7YQ">
    <img src="https://asciinema.org/a/qk7oNQIpDH0nsR0EsnRWsS7YQ.svg" alt="Add Demo" height="256">
  </a>
</p>

## Development

The architecture of Eka is guided by a series of Architectural Decision Records (ADRs). To learn more about the technical details, please refer to the [ADRs](./adrs).

For a detailed breakdown of the development plan, please see the full [ROADMAP.md](./ROADMAP.md).

[atom-nix]: https://github.com/ekala-project/atom
