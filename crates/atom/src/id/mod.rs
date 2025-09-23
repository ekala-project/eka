//! # Atom Identification Constructs
//!
//! This module contains the foundational types and logic for working with Atom
//! identifiers in a cryptographically secure namespace. This enables universal,
//! collision-resistant addressing of software packages (atoms) across sources,
//! with end-to-end integrity from origin to content.
//!
//! ## High-Level Vision
//!
//! The system is designed to create a layered, cryptographic address space for atoms,
//! allowing unambiguous identification and retrieval across diverse repositories or
//! sources. At a conceptual level, this involves:
//! - An immutable **origin** identifier (e.g., a repository's root commit hash) to anchor the
//!   namespace and ensure domain separation.
//! - A human-readable **tag** (moniker) within that origin, validated for descriptiveness and
//!   safety while enabling a vast Unicode-based character set within a single origin.
//! - A machine-readable **id** combining the origin and tag into a globally unique identifier,
//!   represented as a cryptographic **hash** derived from the components of the id using BLAKE3
//!   (with the origin serving as a key in derivation), ensuring atoms with the same tag in
//!   different origins are cryptographically distinct.
//!
//!
//! These primitives, coupled with the rest of an atom's components, enable diverse and efficient
//! tooling capable of unambigiously indexing, querying and addressing software packages with
//! cryptographically sound provenance meta-data from origin, to package identifier, to specific
//! versions and their contents (e.g. via git content hashes).
//!
//! ## Key Concepts
//!
//! **Atom Tags** are Unicode identifiers that descriptively label
//! atoms within an origin. They are validated to ensure they contain only safe
//! characters and contribute to a vast address space for cryptographic disambiguation.
//!
//! **Atom Ids** are the Rust struct coupling a tag to its origin, ultimately represented by the
//! BLAKE3-derived hash these components, providing a cryptographically secure, collision-resistant,
//! and stable identifier for the atom itself. This ensures disambiguation across origins without
//! tying directly to version-specific content (which may be handled in higher layers).
//!
//! ## Tag Validation Rules
//!
//! Atom Tags are validated on construction to ensure they serve as descriptive identifiers while
//! providing a vast character set per origin, suitable for use as the human-readable component of
//! an atom's cryptographic identity. This allows for meaningful Unicode characters across languages
//! (beyond just ASCII/English) without permitting nonsensical or overly permissive
//! content. Validation leverages Unicode general categories for letters and numbers.
//!
//! Atom Tags must:
//! - Be valid UTF-8 encoded Unicode strings
//! - Not exceed 128 bytes in length (measured in UTF-8 bytes)
//! - Not be empty
//! - Start with a Unicode letter (general categories: UppercaseLetter [Lu], LowercaseLetter [Ll],
//!   TitlecaseLetter [Lt], ModifierLetter [Lm], or OtherLetter [Lo]; not a number, underscore, or
//!   hyphen)
//! - Contain only Unicode letters (as defined above), Unicode numbers (DecimalNumber [Nd] or
//!   LetterNumber [Nl]), hyphens (`-`), and underscores (`_`)
//!
//! ## Usage Example
//!
//! ```rust,no_run
//! use atom::store::git::Root;
//! use atom::{AtomId, AtomTag, Compute, Origin};
//!
//! // Create a validated atom tag
//! let tag = AtomTag::try_from("my-atom").unwrap();
//!
//! // Create an AtomId with a Git origin
//! let repo = gix::open(".").unwrap();
//! let commit = repo
//!     .rev_parse_single("HEAD")
//!     .map(|s| repo.find_commit(s))
//!     .unwrap()
//!     .unwrap();
//!
//! let id = AtomId::construct(&commit, tag).unwrap();
//!
//! // Get the has for disambiguated identification
//! let hash = id.compute_hash();
//! println!("Atom fingerprint: {}", hash);
//! ```
//!
//! ## TOML Configuration Example
//!
//! ```toml
//! [atom]
//! tag = "my-atom"
//! ```
#[cfg(test)]
mod tests;

use std::borrow::Borrow;
use std::ffi::OsStr;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;
use unic_ucd_category::GeneralCategory;

const ID_MAX: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String")]
/// A vetted String suitable for an atom's `tag` field
pub struct AtomTag(String);

#[derive(Error, Debug, PartialEq, Eq)]
pub enum Error {
    #[error("An Atom id cannot be more than {} bytes", ID_MAX)]
    TooLong,
    #[error("An Atom id cannot be empty")]
    Empty,
    #[error("An Atom id cannot start with: '{0}'")]
    InvalidStart(char),
    #[error("The Atom id contains invalid characters: '{0}'")]
    InvalidCharacters(String),
    #[error("An Atom id must be valid unicode")]
    InvalidUnicode,
}

/// Trait for computing BLAKE3 hashes of AtomIds.
///
/// This trait is implemented for AtomId to provide a way to compute
/// cryptographically secure hashes that can be used as unique identifiers
/// for atoms in storage backends.
pub trait Compute<'id, T>: Borrow<[u8]> {
    /// Computes the BLAKE3 hash of this AtomId.
    ///
    /// The hash is computed using a key derived from the atom's root value,
    /// ensuring that atoms with the same ID but different roots produce
    /// different hashes.
    ///
    /// # Returns
    ///
    /// An `IdHash` containing the 32-byte BLAKE3 hash and a reference
    /// to the original AtomId.
    fn compute_hash(&'id self) -> IdHash<'id, T>;
}

/// This trait must be implemented to construct new instances of an an [`AtomId`].
/// It tells the [`AtomId::construct`] constructor how to calculate the value for
/// its `root` field.
pub trait Origin<R> {
    /// The error type returned by the [`Origin::calculate_origin`] method.
    type Error;
    /// The method used the calculate the root field for the [`AtomId`].
    ///
    /// # Errors
    ///
    /// This function will return an error if the calculation fails or is impossible.
    fn calculate_origin(&self) -> Result<R, Self::Error>;
}

/// The type representing all the components necessary to serve as
/// an unambiguous identifier. Atoms consist of a human-readable
/// Unicode identifier, as well as a root field, which varies for
/// each store implementation. For example, Git uses the oldest
/// commit in a repositories history.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AtomId<R> {
    origin: R,
    tag: AtomTag,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// Represents the BLAKE3 hash of an AtomId.
///
/// This struct contains a 32-byte BLAKE3 hash that serves as a
/// cryptographically secure, globally unique identifier for an atom.
/// The hash is computed from the combination of the atom's human-readable
/// ID and its context-specific origin value.
pub struct IdHash<'id, T> {
    /// The 32-byte BLAKE3 hash value
    hash: [u8; 32],
    /// Reference to the AtomId that was hashed
    id: &'id AtomId<T>,
}

impl<R> Serialize for AtomId<R> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize only the `tag` field as a string
        self.tag.serialize(serializer)
    }
}

impl<T> Deref for IdHash<'_, T> {
    type Target = [u8; 32];

    fn deref(&self) -> &Self::Target {
        &self.hash
    }
}

impl<'id, R: AsRef<[u8]>> Compute<'id, R> for AtomId<R> {
    fn compute_hash(&'id self) -> IdHash<'id, R> {
        use blake3::Hasher;

        let key = blake3::derive_key("AtomId", self.origin.as_ref());

        let mut hasher = Hasher::new_keyed(&key);
        hasher.update(self.tag.as_bytes());
        IdHash {
            hash: *hasher.finalize().as_bytes(),
            id: self,
        }
    }
}

impl<T> Borrow<[u8]> for AtomId<T> {
    fn borrow(&self) -> &[u8] {
        self.tag.as_bytes()
    }
}

impl<R> AtomId<R>
where
    for<'id> AtomId<R>: Compute<'id, R>,
{
    /// Compute an atom's origin and construct its ID. This method takes a `src`
    /// type which must implement the [`Origin`] struct.
    ///
    /// # Errors
    ///
    /// This function will return an error if the call to
    /// [`Origin::calculate_origin`] fails.
    pub fn construct<T>(src: &T, tag: AtomTag) -> Result<Self, T::Error>
    where
        T: Origin<R>,
    {
        let origin = src.calculate_origin()?;
        Ok(AtomId { origin, tag })
    }

    /// The root field, which serves as a derived key for the blake-3 hash used to
    /// identify the Atom in backend implementations.
    pub fn root(&self) -> &R {
        &self.origin
    }
}

impl AtomTag {
    fn validate_start(c: char) -> Result<(), Error> {
        if AtomTag::is_invalid_start(c) {
            return Err(Error::InvalidStart(c));
        }
        Ok(())
    }

    pub(super) fn validate(s: &str) -> Result<(), Error> {
        if s.len() > ID_MAX {
            return Err(Error::TooLong);
        }

        match s.chars().next().map(AtomTag::validate_start) {
            Some(Ok(())) => (),
            Some(Err(e)) => return Err(e),
            None => return Err(Error::Empty),
        }

        let invalid_chars: String = s.chars().filter(|&c| !AtomTag::is_valid_char(c)).collect();

        if !invalid_chars.is_empty() {
            return Err(Error::InvalidCharacters(invalid_chars));
        }

        Ok(())
    }

    pub(super) fn is_invalid_start(c: char) -> bool {
        matches!(
            GeneralCategory::of(c),
            GeneralCategory::DecimalNumber | GeneralCategory::LetterNumber
        ) || c == '_'
            || c == '-'
            || !AtomTag::is_valid_char(c)
    }

    pub(super) fn is_valid_char(c: char) -> bool {
        matches!(
            GeneralCategory::of(c),
            GeneralCategory::LowercaseLetter
                | GeneralCategory::UppercaseLetter
                | GeneralCategory::TitlecaseLetter
                | GeneralCategory::ModifierLetter
                | GeneralCategory::OtherLetter
                | GeneralCategory::DecimalNumber
                | GeneralCategory::LetterNumber
        ) || c == '-'
            || c == '_'
    }
}

impl Deref for AtomTag {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for AtomTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl FromStr for AtomTag {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        AtomTag::validate(s)?;
        Ok(AtomTag(s.to_string()))
    }
}

impl TryFrom<String> for AtomTag {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        AtomTag::validate(&s)?;
        Ok(AtomTag(s))
    }
}

impl TryFrom<&OsStr> for AtomTag {
    type Error = Error;

    fn try_from(s: &OsStr) -> Result<Self, Self::Error> {
        let s = s.to_str().ok_or(Error::InvalidUnicode)?;
        AtomTag::from_str(s)
    }
}

impl TryFrom<&str> for AtomTag {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        AtomTag::from_str(s)
    }
}

use std::fmt::Display;

impl<R> AtomId<R> {
    /// Return a reference to the Atom's Unicode identifier.
    pub fn tag(&self) -> &AtomTag {
        &self.tag
    }
}

impl<R> Display for AtomId<R>
where
    for<'id> AtomId<R>: Compute<'id, R>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.compute_hash();
        if let Some(max_width) = f.precision() {
            write!(f, "{s:.max_width$}")
        } else {
            write!(f, "{s}")
        }
    }
}

impl<'a, R> Display for IdHash<'a, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = base32::encode(crate::BASE32, &self.hash);
        if let Some(max_width) = f.precision() {
            write!(f, "{s:.max_width$}")
        } else {
            f.write_str(&s)
        }
    }
}
