//! # Atom Crate
//!
//! The `atom` crate provides the core functionality for working with the Atom Format,
//! a key component of the Ekala Project. This format enables the reproducible
//! packaging of select sources from a larger history, making it ideal for
//! dependency management and software distribution.
//!
//! ## Key Concepts
//!
//! **Atoms** are self-contained, reproducible packages that capture a specific
//! version of source code or configuration. They are designed to be:
//! - **Cheap to transfer** over networks
//! - **Trivial to verify** directly from source
//! - **Completely reproducible** across different environments
//!
//! **Lockfiles** capture the exact versions and revisions of all dependencies,
//! ensuring that builds are deterministic and can be reproduced reliably.
//!
//! ## Architecture
//!
//! The crate is organized into several key modules:
//! - [`manifest`] - Defines the Atom manifest format and dependency specification.
//! - [`lock`] - Manages the lockfile format for capturing resolved dependencies.
//! - [`id`] - Handles Atom identification, hashing, and origin tracking.
//! - [`uri`] - Provides tools for Atom URI parsing and resolution.
//! - [`store`] - Implements storage backends for atoms, such as Git.
//! - [`publish`] - Contains logic for publishing atoms to various stores.
//!
//! ## Git Storage Example
//!
//! The current implementation uses Git as the primary storage backend. Atoms are
//! stored as Git refs pointing to orphaned histories:
//!
//! ```console
//! ❯ git ls-remote
//! From https://github.com/ekala-project/eka
//! ceebaca6d44c4cda555db3fbf687c0604c4818eb        refs/eka/atoms/ひらがな/0.1.0
//! a87bff5ae43894a158dadf40938c775cb5b62d4b        refs/eka/meta/ひらがな/0.1.0/manifest
//! 9f17c8c816bd1de6f8aa9c037d1b529212ab2a02        refs/eka/meta/ひらがな/0.1.0/origin
//! ```
//!
//! - The ref under `eka/atoms` points to the complete atom contents.
//! - The `manifest` ref points to a minimal tree containing only the manifest.
//! - The `origin` ref points to the original source commit for verification.
//!
//! ## Basic Usage
//!
//! ```rust,no_run
//! use atom::{Atom, Label, Manifest};
//! use semver::Version;
//!
//! // Create a new atom manifest
//! let manifest = Manifest::new(Label::try_from("my-atom").unwrap(), Version::new(1, 0, 0));
//! ```
//!
//! ## Features
//!
//! - **Type-safe dependency management** with compile-time validation.
//! - **Multiple storage backends** (Git, with extensibility for others).
//! - **Cross-platform compatibility** with TOML-based serialization.
//! - **Comprehensive error handling** with detailed error types.
//! - **Efficient caching** for remote operations.

#![deny(missing_docs)]

use std::sync::LazyLock;

pub use self::core::Atom;
pub use self::id::{AtomId, Compute, Label, Origin};
pub use self::lock::Lockfile;
pub use self::manifest::Manifest;
pub use self::manifest::deps::ManifestWriter;
pub use self::publish::ATOM_REFS;

mod core;
pub mod id;
pub mod lock;
pub mod log;
pub mod manifest;
pub mod publish;
pub mod store;
pub mod uri;

const EKALA: &str = "ekala";
const ATOM: &str = "atom";
/// The base32 alphabet used for encoding Atom hashes.
///
/// This uses the RFC4648 hex alphabet without padding, which provides a good balance
/// between readability and compactness for Atom identifiers.
const BASE32: base32::Alphabet = base32::Alphabet::Rfc4648HexLower { padding: false };
const LOCK: &str = "lock";
const TOML: &str = "toml";

/// The conventional filename for an Atom lockfile (e.g., `atom.lock`).
///
/// This static variable is lazily initialized to ensure it is constructed only when needed.
pub static LOCK_NAME: LazyLock<String> = LazyLock::new(|| format!("{}.{}", ATOM, LOCK));
/// The conventional filename for an Atom manifest (e.g., `atom.toml`).
///
/// This static variable is lazily initialized to ensure it is constructed only when needed.
pub static ATOM_MANIFEST_NAME: LazyLock<String> = LazyLock::new(|| format!("{}.{}", ATOM, TOML));

/// The conventional filename for an Ekala manifest (e.g., `ekala.toml`).
///
/// This static variable is lazily initialized to ensure it is constructed only when needed.
pub static EKALA_MANIFEST_NAME: LazyLock<String> = LazyLock::new(|| format!("{}.{}", EKALA, TOML));
