# Roadmap

This living document outlines the development roadmap for `eka`, `atom-nix`, and eventually `eos`. The immediate goal is to achieve a usable user demo, with an eye towards a stable 1.0 release. To follow along with more specific architectural decisions, checkout the [ADRs](./adrs).

## Milestone: Usable Demo

### Core Atom Format & Publishing

- [x] **Atom Format:** Implement the core data structure for atoms. A detached (from history) reference pointing to only relevant (to the atom) underlying git objects.
- [x] **Git-Native Publishing:** Atoms are published directly to a git repository, leveraging git's object model for storage and transport, as detailed in [ADR #4](adrs/0004-publish-command.md).
  - [x] **Source History Integrity:** All atoms must derive from a common root commit (the first commit in the repository), ensuring a single, verifiable source of truth for an entire project's dependency graph. This root commit's hash is used as a global identifier for the atom set.
  - [x] **Cryptographically Secure & Content-Addressed IDs:** Implement unique atom IDs derived from their declared Unicode tags and a key from the repository's root commit. This allows for unambiguous, cryptographically secure tracking of atoms across projects and ensures that changing a tag correctly changes the atom's identity.
  - [x] **Temporal Conflict Resolution:** Enforce that no two atoms within the same commit (the same point in history) can share the same tag, preventing namespace collisions.
  - [x] **Verifiable, GC-Proof Publishing:** Publish atoms directly from git tree objects, ensuring the published content is an exact, verifiable representation of committed source code. A git reference is created under `refs/eka/meta/<tag>` pointing back to the source commit, protecting it from garbage collection.
  - [x] **Reproducible Atom Commits:** Atom commit headers are augmented with the source path and content hash. The commit timestamp is held constant to ensure the final atom commit hash is fully reproducible.
  - [x] **Efficient Version Discovery:** Publish atoms to versioned tags under `refs/eka/atoms/<tag>/<version>`. This allows for extremely cheap and efficient discovery of available atom versions by querying git references, often with server-side filtering, without needing to fetch any git objects.

### Configuration & Dependency Management

- [x] **Configuration:** Implement parsing for a basic `eka.toml` configuration file.
- [x] **Atom URIs & Aliases:** Implement a user-friendly URI scheme for referencing atoms, with support for aliases in the configuration file for both atom URIs and legacy URLs, as detailed in [ADR #5](adrs/0005-uri-format.md).
- [x] **`eka add` Command:** Implement the `eka add` command for adding dependencies, as detailed in [ADR #2](adrs/0002-eka-add-command.md).
  - [x] **Manifest & Lock File Synchronization:** Ensure atomic updates to the manifest (`eka.toml`) and lock file (`eka.lock`) to maintain consistency, as detailed in [ADR #1](adrs/0001-lock-generation.md) and its [addendum](adrs/0001-lock-generation-addendum.md).
  - [x] **Semantic Version Resolution:** Implement semver-based resolution by querying git references on the remote. The highest matching version is resolved and the exact version, revision hash, and cryptographic ID are recorded in the lock file.
- [ ] **`eka add` Legacy Support:** Finalize support for locking legacy pin-style dependencies (e.g., Nix flakes URLs) to facilitate interoperability, as outlined in [ADR #3](adrs/0003-pure-rust-pin-dependencies.md).
- [ ] **`eka resolve` Command:** Implement a command to synchronize the lock file with the manifest without adding new dependencies.

### Evaluation & Execution

- [ ] **User Entrypoint:** Design and implement a basic user entrypoint for invoking atom evaluations. This will include evaluation-time sandboxing to provide strong reproducibility guarantees.
- [ ] **`atom-nix` Integration:** Integrate the `atom-nix` module system into the main repository, establishing a monorepo structure.
  - [x] **Workable Instantiation:** A usable version of the `atom-nix` module system is available.
  - [x] **Lock Fetcher:** Implemented a working lock fetcher for `atom-nix` to evaluate dependencies produced by `eka`.
  - [ ] **Configuration Interface:** Define and implement an interface for passing configuration from `eka` into the `atom-nix` evaluation, while keeping the interface generic enough to support alternative Nix layouts.
  - [ ] **Config Normalization & Hashing:** Normalize passed configuration (e.g., to a canonical JSON format) and hash it. This ensures that the final artifact's integrity is cryptographically tied to the atom's ID, version, and its specific configuration.

---

## Milestone: Toward Stability

- [ ] **Stable On-Disk Format (v1):** Define and stabilize a v1 of the on-disk format, including git references and atom commit metadata, to ensure backward compatibility for future iterations.
- [ ] **`eos` Evaluation Backend:** Design and implement `eos`, a distributed evaluation and build scheduler that leverages the `snix` ecosystem.
  - [ ] **Leverage `snix`:** `eos` will act as a high-level entrypoint for scheduling and metadata handling, delegating the low-level, content-addressed evaluation and build tasks to the specialized tools in the `snix` ecosystem.
  - [ ] **High-Level API:** Define a client/server interface between `eka` (the user-facing client) and `eos` (the backend) for querying atom metadata, requesting builds, and fetching artifacts.
  - [ ] **Evaluation & Build Queues:** `eos` will manage scheduling evaluations and subsequent builds, responding to `eka` with build status or known artifact locations.
  - [ ] **Metadata Aggregation:** `eos` will manage atom metadata from multiple sources, enabling discovery and search.
- [ ] **E2E Integrity:** Establish a strong cryptographic link from the original source code to the final artifact. Since git's default SHA-1 is not cryptographically sound, a more secure hashing strategy will be required to ensure end-to-end verifiability.
  - [ ] **Secure Hashing:** Rehash the git blobs and trees that constitute an atom using a secure algorithm like BLAKE3.
  - [ ] **Integrity Reference:** Store this secure hash in the atom's commit header and in a new git reference (`refs/eka/meta/<atom-tag>/<version>/<blake3-content-sum>`) pointing to the atom commit.
- [ ] **`eka verify` Command:** Implement a command to ensure the integrity of an atom and its dependencies.
  - [ ] **Source Verification:** The command will download the original source tree and verify that the git tree objects match the content hash advertised by the atom.
  - [ ] **Recursive Verification:** The verification process will be recursive, ensuring the integrity of the entire dependency tree.
- [ ] **Generic Atom Format:** Generalize the atom format to become a generic source code publishing solution, independent of `atom.toml` and `atom.lock`. The goal is to publish versioned source code from any ecosystem (e.g., Cargo crates, Node packages) as atoms, simplifying decentralized distribution and reducing the need for ecosystem-specific glue code (e.g., `*2nix` tools).
- [ ] **Deep Dependency Resolution:** Implement a deep, recursive dependency resolution strategy.
  - [ ] **Recursive Manifest Fetching:** `eka` will learn to recursively fetch only the manifest blobs from remote repositories to build a complete dependency graph.
  - [ ] **Advanced Resolution Algorithm:** Implement a resolution algorithm (e.g., MaxSAT) to find a minimal set of dependency versions that satisfies the entire dependency tree, perhaps allowing for multiple versions of the same dependency if necessary to resolve conflicts.
- [ ] **Ekala Ecosystem Integration:**
  - [ ] **Atomize `ekapkgs`:** Begin experimenting with atomizing the `ekapkgs` project, starting with `corepkgs`, to bootstrap a minimal system of dependencies and gain insights into the atom format's limits & requirements.
- [ ] **Plugin System:** Design and implement a plugin system to extend `eka`'s functionality in a disciplined manner.
- [ ] **Distributed Trust via Signed Tags:** Implement a mechanism for storing build metadata in signed git tag objects.
  - [ ] **Artifact Metadata:** After a successful build, create a signed git tag pointing to the atom commit. This tag will contain metadata linking the atom, version, and configuration hash to the final artifact's output and content hashes.
  - [ ] **Decentralized Trust:** This allows anyone trusting the signing key to retrieve the final artifact from any source (decentralized) without needing to build it themselves, verifying it against the metadata in the signed tag.

---

## Long-Term Vision

- [ ] **Decentralized `eos` Network:** Evolve `eos` into a fully decentralized, peer-to-peer scheduler daemon, allowing a network of instances to share metadata and build artifacts trustlessly.
