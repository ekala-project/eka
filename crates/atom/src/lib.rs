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
//! - [`package`] - Core package management functionality including manifests, lockfiles, and
//!   metadata
//! - [`id`] - Handles Atom identification, hashing, and origin tracking
//! - [`uri`] - Provides tools for Atom URI parsing and resolution
//! - [`storage`] - Implements storage backends for atoms, such as Git
//! - [`log`] - Utility functions for progress indicators and logging
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
//! ## Manifest Structure
//!
//! Atom manifests include a required `[compose]` table that defines the atom's
//! composer. The composer specifies which atom from a set provides the import
//! functionality for this atom.
//!
//! The `ValidManifest` type is the publicly exposed variant that includes
//! post-deserialization validation to ensure manifest consistency. The `Manifest`
//! type is a private implementation detail used internally.
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

pub use self::id::{AtomId, Compute, Label, Origin};
pub use self::package::metadata::lock::Lockfile;
pub use self::package::metadata::{Atom, EkalaManager};
pub use self::package::publish::ATOM_REFS;
pub use self::package::{ManifestWriter, ValidManifest};

pub mod id;
pub mod log;
pub mod package;
pub mod storage;
pub mod uri;

// Sets compile time constants
eka_root_macro::eka_origin_info!();

const ATOM: &str = "atom";
/// The base32 alphabet used for encoding Atom hashes.
///
/// This uses the RFC4648 hex alphabet without padding, which provides a good balance
/// between readability and compactness for Atom identifiers.
const BASE32: base32::Alphabet = base32::Alphabet::Rfc4648HexLower { padding: false };
const EKALA: &str = "ekala";
const LOCK: &str = "lock";
const TOML: &str = "toml";

/// The conventional filename for an Atom lockfile (e.g., `atom.lock`).
///
/// This static variable is lazily initialized to ensure it is constructed only when needed.
pub static ATOM_MANIFEST_NAME: LazyLock<String> = LazyLock::new(|| format!("{}.{}", ATOM, TOML));
/// The conventional filename for an Atom manifest (e.g., `atom.toml`).
///
/// This static variable is lazily initialized to ensure it is constructed only when needed.
pub static EKALA_MANIFEST_NAME: LazyLock<String> = LazyLock::new(|| format!("{}.{}", EKALA, TOML));
/// The conventional filename for an Ekala manifest (e.g., `ekala.toml`).
///
/// This static variable is lazily initialized to ensure it is constructed only when needed.
pub static LOCK_NAME: LazyLock<String> = LazyLock::new(|| format!("{}.{}", ATOM, LOCK));

/// A type alias for a boxed error that is sendable and syncable.
type BoxError = Box<dyn std::error::Error + Send + Sync>;
