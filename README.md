# Eka: A New Foundation for the Software Supply Chain

> ⚠️ **Warning:** Eka is still in early stages. Features are unstable and subject to change.

A command-line tool for decentralized software dependency management using the Atom Protocol.

[![standard-readme compliant](https://img.shields.io/badge/readme%20style-standard-brightgreen.svg?style=flat-square)](https://github.com/RichardLitt/standard-readme)

## Table of Contents

- [Background](#background)
- [Install](#install)
- [Usage](#usage)
- [API](#api)
- [Contributing](#contributing)
- [License](#license)

## Background

This is the first step towards a more resilient and transparent software supply chain, free from the single points of failure inherent in traditional, centralized package registries.

### What is the Atom Protocol?

The [Atom Protocol](https://docs.eka.rs/atom/) represents a fundamental rethinking of software dependency management, moving beyond traditional package registries to create a decentralized, cryptographically-secure foundation for the software supply chain. At its heart lies a new standard that treats software packages as verifiable, immutable slices of Git repositories. This approach eliminates single points of failure while providing mathematical guarantees of integrity and reproducibility.

The protocol addresses the inherent limitations of centralized package registries by focusing on three core principles:

- **Decentralized Distribution:** Instead of a central server like npm or PyPI, Atom uses Git repositories as the source of truth. It leverages the distributed nature of Git to ensure that package availability is not tied to a single entity, eliminating a critical vulnerability in the software supply chain.

- **Source as Truth:** Instead of copying source code into a registry, atoms are lightweight references to the same Git objects that comprise the original source code. This creates an unbreakable link between published packages and their origins, ensuring that the packaged code is always identical to the source.

- **No Single Points of Failure:** Dependencies can be resolved from multiple mirrors or the original repository, ensuring availability even if one source becomes unavailable. This distributed approach means that a single registry outage or compromise cannot halt development.

- **Community-Driven Resilience:** Anyone can mirror an atom repository, creating a distributed network that cannot be taken down by any single entity. This community-driven approach ensures long-term availability and prevents vendor lock-in.

- **Practical Implications:** In a world where incidents like the `left-pad` npm package breaking the internet or the `xz-utils` backdoor demonstrate the fragility of centralized systems, Eka's decentralized approach ensures that critical dependencies remain available and verifiable, even during network outages or registry compromises.

- **Designed for Efficiency:** By creating unambiguous, content-addressed cryptographic IDs for every package, Atom enables a future for highly efficient, decentralized build pipelines. This foundation allows for a system that is not only more secure and resilient but is also designed for high-performance, distributed build systems.

### The Nix Connection: A Complete, Reproducible Workflow

A verifiable, git-native source packaging format is only half of the story. To achieve true end-to-end supply chain security, the integrity of the source code must be translated into a build artifact from a reproducible build process. This requires a deterministic build system that can guarantee identical inputs—a core strength of Nix.

`eka`'s inaugural implementation is therefore deeply integrated with the **Nix ecosystem**. The architecture is best understood as a clean separation of concerns, with `eka` acting as the unified user-facing client for the entire workflow:

- **Source Code Management (The Atom Protocol):** This is the foundational layer, concerned with the verifiable, decentralized distribution of _source code_. Its ambition is to be a universal, language-agnostic standard.

- **Artifact Build System (Nix / Eos):** This is the backend, responsible for taking locked source dependencies and producing a final software artifact. The long-term vision is for this to be handled by **Eos**, a distributed build scheduler that will eventually power Eka's evaluation backend.

`eka` is the package manager that bridges these two layers. Its primary expertise is managing _source code_ dependencies to produce a locked set of inputs. However, it is also a package manager in the traditional sense, as it will communicate with the build system (`Eos`) to build, fetch, and install the final _artifacts_, providing a seamless experience for developers.

### The Ekala Ecosystem

This work is centered on four core components, which will eventually be unified into a single monorepo:

- **Eka:** A user-facing CLI that provides a reasonable, statically-determinable interface for managing dependencies and builds.
- **[atom-nix]:** A Nix module system for evaluating atoms.
- **Atom Format:** A verifiable, versioned, and git-native format for publishing source code, designed for decentralized distribution and end-to-end integrity.
- **Eos (Future):** A planned distributed, content-addressed build scheduler that will eventually power Eka's evaluation backend.

### Design Goals

- **Disciplined:** Eka maintains a clean separation of concerns. It is an expert at managing source code dependencies, while delegating the heavy lifting of evaluation and building to a dedicated backend like Nix or Eos.
- **Fast:** Dependency management commands in `eka` are designed to be exceptionally fast, operating primarily on static metadata. Querying, resolving, and locking atoms are near-instantaneous operations.
- **Conceptually High-Level:** Developers care about packages, versions, and reproducibility, not the low-level details of Nix derivations. Eka provides an interface that speaks to developers at their level of concern, abstracting away the complexity of the underlying build system.
- **User-Centric:** No matter how well designed a system's architecture, if it's painful to use, it will fail. Eka's philosophy integrates user experience as a core part of the development cycle, augmenting rather than opposing efficiency and performance. This harmonious balance of interface and technical excellence enables highly efficient implementations. For example, by carefully considering how mirrors were presented to users, we landed on an elegant data model that made efficient asynchronous resolution across many mirrors trivially efficient—the prototype model would have made this difficult or impossible. This demonstrates how UX and efficiency are directly coupled through thoughtful data modeling.

For more information about the project's vision and roadmap, see the [ROADMAP.md](./ROADMAP.md).

### Talk: "Nix Sucks, Everything Else is Worse"

For a deeper dive into the problems Eka aims to solve and the technical foundations behind it, watch the talk ["Nix Sucks, Everything Else is Worse"](https://odysee.com/@nrdxp:6/Nix-Sucks-Everything-else-is-Worse:4?r=dKZibSSnzMGP3T5e5whD2QmoMj1AUijf) by Tim DeHerrera, the creator of Eka.

## Install

### Prerequisites

- [Nix](https://nixos.org/download.html) (required for the build environment)

### From Source

Building Eka requires specific dependencies. The easiest way to set everything up properly, including the Rust compiler, is to use the provided Nix shell:

```bash
git clone https://github.com/ekala-project/eka.git
cd eka
nix-shell ./dev # or `direnv allow`, if you prefer
# Inside the shell:
cargo build --release
# Binary will be at target/release/eka
```

### Development Environment

The Nix shell provides all necessary dependencies including:

- Exact Rust version (as specified in `rust-toolchain.toml`)
- snix and protocol buffer dependencies
- All required build tools

```bash
nix-shell ./dev
# or with direnv: direnv allow
```

## Usage

### Basic Commands

Initialize a new Eka package set:

```bash
eka init
```

Create a new atom, and at it to the set:

```bash
eka new demo-app
cd demo-app
```

Add dependencies using Atom URIs:

```bash
eka add gh:nrdxp/home::dev
eka add gh:nrdxp/home::hosts
```

Add direct Nix dependencies:

```bash
eka add direct nix pkgs --git nixpkgs-unstable
```

Resolve and lock dependencies:

```bash
eka resolve
```

Publish atoms:

```bash
eka publish demo-app
```

### Example Project Structure

After initialization, your project will have:

```
demo-app/
├── atom.toml      # Manifest file
├── atom.lock      # Lock file
└── src/           # Your source code
```

### Manifest Example

```toml
[package]
label = "demo-app"
version = "0.1.0"

[package.sets]
company-atoms = "git@github.com:nrdxp/home"

[deps.from.company-atoms]
dev = "^1.0"
hosts = "^1.0"

[deps.direct.nix]
pkgs = { git = "https://github.com/NixOS/nixpkgs", ref = "nixos-unstable" }
```

### Atom URIs: User-Friendly Addressing

While the underlying system uses cryptographic IDs for security, developers interact with atoms through intuitive URIs that abstract away the complexity:

- `gh:owner/repo::atom-name@^1.0` - GitHub repository with semantic versioning
- `gl:group/project::library` - GitLab addressing
- `company-atoms::auth-lib` - Custom alias for internal repositories

## API

### Rust Library

The `atom` crate provides a comprehensive Rust API for working with the Atom Protocol:

```rust
use atom::{AtomId, Lockfile, ValidManifest, EkalaManager};

// Core types for Atom management
let atom_id: AtomId = /* ... */;
let lockfile: Lockfile = /* ... */;
let manifest: ValidManifest = /* ... */;
let manager: EkalaManager = /* ... */;
```

Key modules include:

- `atom::id` - Atom identification and cryptographic hashing
- `atom::package` - Manifest and lockfile management
- `atom::uri` - URI parsing and resolution
- `atom::storage` - Storage backend implementations

See the [atom crate documentation](https://docs.rs/atom) for detailed API information.

### CLI Interface

Eka provides a command-line interface built on top of the atom library:

```bash
eka --help  # Show available commands
```

See the [CLI reference](./docs/reference/cli-reference.md) for detailed command information.

## Contributing

Please read through our [contributing guidelines](./CONTRIBUTING.md). Included are directions for opening issues, coding standards, and notes on development.

Join our [Discord server](https://discord.gg/DgC9Snxmg7) for informal chat and collaboration.

## License

GNU General Public License v3.0 or later

See [LICENSE](./LICENSE) for the full license text.
