//! # Atom Publishing
//!
//! This module provides the types and logic necessary to efficienctly publish Atoms
//! to a store implementation. Currently, only a Git store is implemented, but future
//! work will likely include more alternate backends, e.g. an S3 bucket.
pub mod error;
#[cfg(feature = "git")]
pub mod git;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[cfg(feature = "git")]
use git::GitContent;

use crate::AtomId;
use crate::id::Id;

/// The results of Atom publishing, for reporting to the user.
pub struct Record<R> {
    id: AtomId<R>,
    content: Content,
}

/// Basic statistics collected during a publishing request.
#[derive(Default)]
pub struct Stats {
    /// How many Atoms were actually published.
    pub published: u32,
    /// How many Atoms were safely skipped because they already existed.
    pub skipped: u32,
    /// How many Atoms failed to publish due to some error condition.
    pub failed: u32,
}

/// A Result is used over an Option here mainly so we can report which
/// Atom was skipped, but it does not represent a true failure condition
type MaybeSkipped<T> = Result<T, Id>;

/// A Record that signifies whether an Atom was published or safetly skipped.
type PublishOutcome<R> = MaybeSkipped<Record<R>>;

/// A [`HashMap`] containing all valid Atoms in the current store.
type ValidAtoms = HashMap<Id, PathBuf>;

/// Contains the content pertinent to a specific implementation for reporting results
/// to the user.
pub enum Content {
    #[cfg(feature = "git")]
    /// Content specific to the Git implementation.
    Git(GitContent),
}

/// A [`Builder`] produces a [`Publish`] implementation, which has no other constructor.
/// This is critical to ensure that vital invariants necessary for maintaining a clean
/// and consistent state in the Ekala store are verified before publishing can occur.
pub trait Builder<'a, R> {
    /// The error type returned by the [`Builder::build`] method.
    type Error;
    /// The [`Publish`] implementation to construct.
    type Publisher: Publish<R>;

    /// Collect all the Atoms in the worktree into a set.
    ///
    /// This function must be called before `Publish::publish` to ensure that there are
    /// no duplicates, as this is the only way to construct an implementation.
    fn build(&self) -> Result<(ValidAtoms, Self::Publisher), Self::Error>;
}

trait StateValidator<R> {
    type Error;
    type Publisher: Publish<R>;
    /// Validate the state of the Atom source.
    ///
    /// This function is called during construction to ensure that we
    /// never allow for an inconsistent state in the final Ekala store.
    ///
    /// Any conditions that would result in an inconsistent state will
    /// result in an error, making it impossible to construct a publisher
    /// until the state is corrected.
    fn validate(publisher: &Self::Publisher) -> Result<ValidAtoms, Self::Error>;
}

mod private {
    /// a marker trait to seal the [`Publish<R>`] trait
    pub trait Sealed {}
}

/// The trait primarily responsible for exposing Atom publishing logic for a given store.
pub trait Publish<R>: private::Sealed {
    /// The error type returned by the publisher.
    type Error;

    /// Publishes Atoms.
    ///
    /// This function processes a collection of paths, each representing an Atom to be published.
    /// Internally the implementation calls [`Publish::publish_atom`] for each path.
    ///
    /// # Error Handling
    /// - The function aims to process all provided paths, even if some fail.
    /// - Errors and skipped Atoms are collected as results but do not halt the overall process.
    /// - The function continues until all the Atoms have been processed.
    ///
    /// # Return Value
    /// Returns a vector of results types, where the outter result represents whether an Atom has
    /// failed, and the inner result determines whether an Atom was safely skipped, e.g. because it
    /// already exists.
    fn publish<C>(&self, paths: C) -> Vec<Result<PublishOutcome<R>, Self::Error>>
    where
        C: IntoIterator<Item = PathBuf>;

    /// Publish an Atom.
    ///
    /// This function takes a single path and publishes the Atom located there, if possible.
    ///
    /// # Return Value
    /// - An outcome is either the record ([`Record<R>`]) of the successfully publish Atom or the
    ///   [`crate::AtomId`] if it was safely skipped.
    ///
    /// - The function will return an error ([`Self::Error`]) if the Atom could not be published for
    ///   any reason, e.g. invalid manifests.
    fn publish_atom<P: AsRef<Path>>(&self, path: P) -> Result<PublishOutcome<R>, Self::Error>;
}

impl<R> Record<R> {
    /// Return a reference to the [`AtomId`] in the record.
    pub fn id(&self) -> &AtomId<R> {
        &self.id
    }

    /// Return a reference to the [`Content`] of the record.
    pub fn content(&self) -> &Content {
        &self.content
    }
}

const EMPTY_SIG: &str = "";
const ATOMIC_ROOT: &str = "eka";
const ATOM_FORMAT_VERSION: &str = "pre1.0";
const ATOM_REF: &str = "atoms";
const ATOM_MANIFEST: &str = "manifest";
const ATOM_META_REF: &str = "meta";
const ATOM_ORIGIN: &str = "origin";
