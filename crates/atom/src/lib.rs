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
//! - [`manifest`] - Atom manifest format and dependency specification
//! - [`lock`] - Lockfile format for capturing resolved dependencies
//! - [`id`] - Atom identification and hashing
//! - [`uri`] - Atom URI parsing and resolution
//! - [`store`] - Storage backends for atoms (Git, etc.)
//! - [`publish`] - Publishing atoms to stores
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
//! - The ref under eka/atoms points to the complete atom contents
//! - The `manifest` ref points to a minimal tree containing only the manifest
//! - The `origin` ref points to the original source commit for verification
//!
//! ## Basic Usage
//!
//! ```rust,no_run
//! use atom::{Atom, AtomTag, Manifest};
//! use semver::Version;
//!
//! // Create a new atom manifest
//! let manifest = Manifest::new(
//!     AtomTag::try_from("my-atom").unwrap(),
//!     Version::new(1, 0, 0),
//!     Some("A sample atom".to_string()),
//! );
//! ```
//!
//! ## Features
//!
//! - **Type-safe dependency management** with compile-time validation
//! - **Multiple storage backends** (Git, with extensibility for others)
//! - **Cross-platform compatibility** with TOML-based serialization
//! - **Comprehensive error handling** with detailed error types
//! - **Efficient caching** for remote operations
#![deny(missing_docs)]
#![cfg_attr(not(feature = "git"), allow(dead_code))]

mod core;
mod id;
pub mod lock;
pub mod manifest;

pub mod publish;
pub mod store;
pub mod uri;
pub use core::Atom;
use std::sync::LazyLock;

pub use id::{AtomId, AtomTag, Compute, Origin};
pub use lock::{Lockfile, ResolutionMode};
pub use manifest::Manifest;

/// The file extension used for Atom manifest files.
const TOML: &str = "toml";

/// The base32 alphabet used for encoding Atom hashes.
///
/// This uses the RFC4648 hex alphabet without padding, which provides a good balance
/// between readability and compactness for Atom identifiers.
const BASE32: base32::Alphabet = base32::Alphabet::Rfc4648HexLower { padding: false };

/// The filename used for Atom manifest files.
///
/// This is constructed by combining the base name "atom" with the TOML extension.
static ATOM_MANIFEST: LazyLock<String> = LazyLock::new(|| format!("atom.{}", crate::TOML));
