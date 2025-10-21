# Eka: A New Foundation for the Software Supply Chain

> ⚠️ **Warning:** Eka is in early experimental stages. Features are unstable and subject to change.

`eka` is a command-line tool for managing software dependencies using the **Atom Protocol**, a new standard for decentralized software distribution. It is designed from the ground up to provide a more efficient, secure, and reproducible development experience.

This is the first step towards a more resilient and transparent software supply chain, free from the single points of failure inherent in traditional, centralized package registries.

## What is the Atom Protocol?

The [Atom Protocol](https://docs.eka.rs/atom/) represents a fundamental rethinking of software dependency management, moving beyond traditional package registries to create a decentralized, cryptographically-secure foundation for the software supply chain. At its heart lies a new standard that treats software packages as verifiable, immutable slices of Git repositories. This approach eliminates single points of failure while providing mathematical guarantees of integrity and reproducibility.

The protocol addresses the inherent limitations of centralized package registries by focusing on three core principles:

- **Decentralized Distribution:** Instead of a central server like npm or PyPI, Atom uses Git repositories as the source of truth. It leverages the distributed nature of Git to ensure that package availability is not tied to a single entity, eliminating a critical vulnerability in the software supply chain.

- **Source as Truth:** Instead of copying source code into a registry, atoms are lightweight references to the same Git objects that comprise the original source code. This creates an unbreakable link between published packages and their origins, ensuring that the packaged code is always identical to the source.

- **No Single Points of Failure:** Dependencies can be resolved from multiple mirrors or the original repository, ensuring availability even if one source becomes unavailable. This distributed approach means that a single registry outage or compromise cannot halt development.

- **Community-Driven Resilience:** Anyone can mirror an atom repository, creating a distributed network that cannot be taken down by any single entity. This community-driven approach ensures long-term availability and prevents vendor lock-in.

- **Practical Implications:** In a world where incidents like the `left-pad` npm package breaking the internet or the `xz-utils` backdoor demonstrate the fragility of centralized systems, Eka's decentralized approach ensures that critical dependencies remain available and verifiable, even during network outages or registry compromises.

- **Designed for Efficiency:** By creating unambiguous, content-addressed cryptographic IDs for every package, Atom enables a future for highly efficient, decentralized build pipelines. This foundation allows for a system that is not only more secure and resilient but is also designed for high-performance, distributed build systems.

## The Nix Connection: A Complete, Reproducible Workflow

A verifiable, git-native source packaging format is only half of the story. To achieve true end-to-end supply chain security, the integrity of the source code must be translated into a build artifact from a reproducible build process. This requires a deterministic build system that can guarantee identical inputs—a core strength of Nix.

`eka`'s inaugural implementation is therefore deeply integrated with the **Nix ecosystem**. The architecture is best understood as a clean separation of concerns, with `eka` acting as the unified user-facing client for the entire workflow:

- **Source Code Management (The Atom Protocol):** This is the foundational layer, concerned with the verifiable, decentralized distribution of _source code_. Its ambition is to be a universal, language-agnostic standard.

- **Artifact Build System (Nix / Eos):** This is the backend, responsible for taking locked source dependencies and producing a final software artifact. The long-term vision is for this to be handled by **Eos**, a distributed build scheduler that can orchestrate builds across multiple machines for maximum efficiency.

`eka` is the package manager that bridges these two layers. Its primary expertise is managing _source code_ dependencies to produce a locked set of inputs. However, it is also a package manager in the traditional sense, as it will communicate with the build system (`Eos`) to build, fetch, and install the final _artifacts_, providing a seamless experience for developers.

## The Ekala Ecosystem

This work is centered on four core components, which will eventually be unified into a single monorepo:

- **Eka:** A user-facing CLI that provides a reasonable, statically-determinable interface for managing dependencies and builds.
- **[atom-nix]:** A Nix module system for evaluating atoms.
- **Atom Format:** A verifiable, versioned, and git-native format for publishing source code, designed for decentralized distribution and end-to-end integrity.
- **Eos (Future):** A planned distributed, content-addressed build scheduler that will eventually power Eka's evaluation backend.

## Design Goals

- **Disciplined:** Eka maintains a clean separation of concerns. It is an expert at managing source code dependencies, while delegating the heavy lifting of evaluation and building to a dedicated backend like Nix or Eos.
- **Fast:** Dependency management commands in `eka` are designed to be exceptionally fast, operating primarily on static metadata. Querying, resolving, and locking atoms are near-instantaneous operations.
- **Conceptually High-Level:** Developers care about packages, versions, and reproducibility, not the low-level details of Nix derivations. Eka provides an interface that speaks to developers at their level of concern, abstracting away the complexity of the underlying build system.

## Core Concepts

### Atom Identity and Cryptographic IDs

At the core of Eka's security model is the concept of **atom identity**—a cryptographically unique identifier that provides mathematical guarantees of authenticity and prevents name collisions across the global namespace.

#### How Atom IDs Work

Every atom has a unique identity derived from two fundamental components:

1. **Repository Identity:** Established through an initialization commit with injected entropy, providing robust disambiguation of forks from mirrors and temporal anchoring for provenance tracking.

2. **Atom Label:** A human-readable identifier (like `serde` or `tokio`) that must be unique within its repository. Labels provide user-friendly naming while the cryptographic hash ensures global uniqueness.

These components are combined using the BLAKE3 cryptographic hash function to create a globally unique identifier.

This construction provides several security properties:

- **Collision Resistance:** The cryptographic hash makes it computationally infeasible to create two different inputs that produce the same ID.
- **Uniqueness:** Each atom has a mathematically unique identity, preventing confusion between packages from different repositories.
- **Verifiability:** The ID can be independently computed and verified by anyone with access to the repository.

#### Example: Resolving Dependencies

Consider two different repositories both containing a package named `utils`:

```toml
# Repository A (github.com/company-a/atoms)
[package]
label = "utils"
version = "1.0.0"

# Repository B (github.com/company-b/atoms)
[package]
label = "utils"
version = "2.0.0"
```

Despite having the same label, these atoms have completely different identities because they originate from different repositories. The cryptographic ID ensures that `company-a/utils` can never be confused with `company-b/utils`, even if both repositories are compromised, renamed, or forked.

**Practical Implications:** This approach eliminates "dependency confusion" attacks where malicious packages with identical names can replace legitimate ones. The cryptographic foundation makes it mathematically impossible for an attacker to create a package that appears legitimate to the system, providing strong security guarantees against supply chain attacks.

### Reproducibility Through Manifests and Locks

Eka achieves true end-to-end reproducibility by separating source code management from build execution, with deterministic build backends translating source integrity into artifact integrity.

#### The Manifest: Declarative Dependencies

The `atom.toml` manifest serves as your project's declarative dependency specification, defining what your project needs without specifying exact versions:

```toml
[package]
label = "my-web-app"
version = "0.1.0"

[package.sets]
company-atoms = "git@github.com:our-company/atoms"
public-atoms = ["https://atoms.example.com", "https://mirror.atoms.example.com"]

[deps.from.company-atoms]
auth-lib = "^2.1"
logging = "^1.0"

[deps.from.public-atoms]
serde = "^1.0"

[deps.direct.nix]
# Direct dependencies for non-atom sources
nixpkgs = { git = "https://github.com/NixOS/nixpkgs", ref = "nixos-unstable" }
```

The manifest supports:

- **Semantic Versioning:** Version constraints like `^2.1` allow automatic updates within compatible ranges.
- **Multiple Sources:** Dependencies can be sourced from different repositories or mirrors.
- **Mixed Ecosystems:** Support for both atom dependencies and direct backend-specific dependencies.

#### The Lockfile: Cryptographic Snapshot

The `atom.lock` file captures the exact resolved state of all dependencies, creating a cryptographic snapshot that ensures reproducible builds:

```toml
version = 1

[sets]
"<hash-of-company-root>" = ["git@github.com:our-company/atoms"]
"<hash-of-public-root>" = ["https://atoms.example.com", "https://mirror.atoms.example.com"]

[[deps]]
type = "atom"
label = "auth-lib"
version = "2.1.3"
set = "<hash-of-company-root>"
rev = "<exact-git-commit>"
id = "<cryptographic-atom-id>"

[[deps]]
type = "atom"
label = "serde"
version = "1.0.42"
set = "<hash-of-public-root>"
rev = "<exact-git-commit>"
id = "<cryptographic-atom-id>"
```

The lockfile provides:

- **Exact Versions:** Pinning to specific versions and commits eliminates ambiguity.
- **Cryptographic Verification:** Each dependency includes its cryptographic ID for integrity verification.
- **Source Tracking:** Records which repository set each dependency came from.

**Practical Implications:** With the lockfile, builds are completely reproducible across different machines, operating systems, and time. The same `atom.lock` will always produce identical artifacts, eliminating "works on my machine" problems and ensuring supply chain security. This reproducibility extends from source code to final binaries, providing end-to-end integrity guarantees.

### Efficiency and Performance

The Atom Protocol is designed for high-performance, decentralized build systems through content-addressed cryptographic IDs and backend-agnostic abstractions. While currently implemented with Git, the core traits ensure compatibility with future version control systems or distributed storage backends.

#### Content-Addressed Efficiency

By using cryptographic hashes as identifiers, the protocol enables:

- **Deduplication:** Identical content is stored only once, reducing storage and bandwidth requirements.
- **Parallel Resolution:** Dependencies can be fetched and verified concurrently without conflicts.
- **Incremental Updates:** Only changed atoms need to be downloaded, minimizing network overhead.

#### Separate History for Atoms

Each atom maintains its own independent history through dedicated references, allowing for efficient tracking of version-specific changes without requiring full repository clones. This design enables:

- **Version Isolation:** Each atom version exists as a self-contained, verifiable unit.
- **Selective Fetching:** Only relevant atom changes need to be retrieved, optimizing performance.
- **Concurrent Processing:** Multiple atoms can be resolved simultaneously across distributed systems.

#### Unique Ref Hierarchy for Fast Discovery

The structured namespace (`refs/ekala/`) optimizes atom discovery:

- **Repository Identity:** `refs/ekala/init` establishes temporal anchoring.
- **Atom Content:** `refs/ekala/atoms/<label>/<version>` provides direct access to atom content.
- **Metadata Separation:** Parallel hierarchies enable efficient querying without expensive traversals.

This architecture enables near-instantaneous local discovery and remote querying, supporting scalable, distributed build pipelines.

#### Backend Agnosticism

The protocol's abstraction layer ensures that fundamental concepts—cryptographic identity, content addressing, and decentralized distribution—are independent of the underlying storage or version control system. Future implementations could leverage distributed hash tables, object storage, or alternative VCS while maintaining identical security and efficiency guarantees.

### Global Namespace Management

Eka's global namespace combines human-readable labels with cryptographic verification, enabling secure, decentralized collaboration across organizations.

#### Repository Identity and Discovery

> **Note:** Repository identity through initialization commits with entropy injection is proposed for future implementation. Currently, repository identity is established through the root commit hash. The `ekala.toml` manifest is now implemented and serves as the single source of truth for a repository's atom composition.

Repository identity is established through an initialization commit with injected entropy, providing robust disambiguation of forks from mirrors. This temporal anchoring ensures that even identical repositories have different identities, preventing confusion between legitimate repositories and malicious forks.

The `ekala.toml` manifest serves as the single source of truth for a repository's atom composition:

```toml
# ekala.toml - Repository manifest
[set]
packages = [
    "path/to/auth-lib",
    "path/to/logging",
    "path/to/ui-components"
]

[metadata]
domain = "our-company.com"
license = "MIT"
tags = ["internal", "production-ready"]
```

This manifest defines:

- **Package Inventory:** Which atoms are available in the repository.
- **Metadata:** Domain, license, and categorization information.
- **Discovery:** Enables automated discovery and indexing of available atoms.

#### Atom URIs: User-Friendly Addressing

While the underlying system uses cryptographic IDs for security, developers interact with atoms through intuitive URIs that abstract away the complexity:

- `gh:owner/repo::atom-name@^1.0` - GitHub repository with semantic versioning
- `gl:group/project::library` - GitLab addressing
- `company-atoms::auth-lib` - Custom alias for internal repositories

These URIs are resolved to cryptographic IDs in the lockfile, ensuring portability and security. The URI system supports:

- **Multiple Platforms:** GitHub, GitLab, and custom repositories.
- **Version Constraints:** Semantic versioning and exact version pinning.
- **Aliases:** Custom names for frequently used repositories.

**Practical Implications:** The global namespace allows seamless collaboration across organizations while maintaining security. Teams can share atoms across different repositories, companies, or even continents, with mathematical guarantees that the right code is being used. The system scales naturally without requiring central coordination or registry maintenance, enabling a truly decentralized ecosystem.

### Why This Matters: Beyond Package Management

Eka is not just another package manager—it's a foundational technology for a more resilient software ecosystem. By combining decentralization, cryptographic security, and reproducible builds, Eka addresses the fundamental vulnerabilities that have plagued software development for decades.

The result is a system where:

- **Security is built-in**, not bolted on through after-the-fact audits
- **Availability is guaranteed** through decentralization and community resilience
- **Reproducibility is automatic** through cryptographic locking and deterministic builds
- **Collaboration is seamless** through global namespace management and intuitive URIs

This foundation enables not just better package management, but a complete rethinking of how we build, distribute, and trust software in an increasingly interconnected world. Eka provides the infrastructure for a software supply chain that is as reliable and secure as the underlying mathematics that power it.

## Development

The architecture of Eka is guided by a series of Architectural Decision Records (ADRs). To learn more about the technical details, please refer to the [ADRs](./adrs).

For a detailed breakdown of the development plan, please see the full [ROADMAP.md](./ROADMAP.md).

[atom-nix]: https://github.com/ekala-project/atom
