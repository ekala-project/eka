//! # Package Management
//!
//! This module provides the core functionality for managing atom packages,
//! including manifest definitions, dependency resolution, publishing, and
//! lockfile management.
//!
//! ## Submodules
//!
//! - [`metadata`] - Core types for atoms, manifests, and lockfiles
//! - [`publish`] - Publishing atoms to storage backends
//! - [`resolve`] - Dependency resolution and synchronization
//! - [`sets`] - Package set management and validation
//!
//! ## Key Types
//!
//! - [`Atom`] - Represents an atom with its metadata and dependencies
//! - [`ValidManifest`] - Publicly exposed manifest type with validation
//! - [`ManifestWriter`] - Writer for atom manifests that ensures lockfile consistency
//! - [`Lockfile`] - Resolved dependency lockfile
//! - [`AtomError`] - Errors that can occur during package operations

pub use metadata::EkalaManifest;
pub use metadata::manifest::{GitSpec, ManifestWriter, ValidManifest};

pub mod publish;

pub(crate) mod metadata;
mod resolve;
mod sets;

/// An error that can occur when parsing or handling an atom manifest.
#[derive(thiserror::Error, Debug)]
pub enum AtomError {
    /// The manifest is missing the required `[atom]` table.
    #[error("Manifest is missing the `[package]` key")]
    Missing,
    /// One of the fields in the `[package]` table is missing or invalid.
    #[error(transparent)]
    InvalidAtom(#[from] toml_edit::de::Error),
    /// The manifest is not valid TOML.
    #[error(transparent)]
    InvalidToml(#[from] toml_edit::TomlError),
    /// could not locate ekala manifest
    #[error("failed to locate Ekala manifest")]
    EkalaManifest,
    /// An I/O error occurred while reading the manifest file.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// An Label is missing or malformed
    #[error(transparent)]
    Id(#[from] crate::id::Error),
    /// A document error
    #[error(transparent)]
    Doc(#[from] metadata::DocError),
    /// A generic boxed error
    #[error(transparent)]
    Generic(#[from] crate::BoxError),
    #[error(transparent)]
    /// A path strip prefix error
    Prefix(#[from] std::path::StripPrefixError),
}
