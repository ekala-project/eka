//! # Atom Identification Constructs
//!
//! This module provides foundational types for creating and managing atom
//! identifiers within a cryptographically secure namespace. It enables universal,
//! collision-resistant addressing of software packages (atoms), ensuring
//! end-to-end integrity from origin to content.
//!
//! ## High-Level Vision
//!
//! The system establishes a layered, cryptographic address space for atoms,
//! facilitating unambiguous identification and retrieval across diverse
//! repositories. This is achieved through:
//!
//! - An immutable **origin** identifier (e.g., a repository's root commit hash) to anchor the
//!   namespace.
//! - A human-readable **tag** (moniker) that is validated for descriptiveness and safety.
//! - A machine-readable **id** that combines the origin and tag into a globally unique identifier,
//!   represented as a BLAKE3-derived cryptographic hash.
//!
//! These primitives support robust tooling for indexing, querying, and addressing
//! software packages with verifiable provenance.
//!
//! ## Key Concepts
//!
//! - **Atom Tags**: Unicode identifiers that label atoms within an origin. They are validated to
//!   ensure they contain only safe, descriptive characters.
//! - **Atom Ids**: A struct coupling a tag to its origin, represented by a BLAKE3-derived hash.
//!   This provides a secure, collision-resistant, and stable identifier for the atom.
//!
//! ## Tag Validation Rules
//!
//! Atom tags must adhere to the following rules:
//! - Be valid UTF-8 encoded Unicode strings.
//! - Not exceed 128 bytes in length.
//! - Not be empty.
//! - Start with a Unicode letter.
//! - Contain only Unicode letters, numbers, hyphens (`-`), and underscores (`_`).
//!
//! ## Usage Example
//!
//! ```rust,no_run
//! use atom::store::git::Root;
//! use atom::{AtomId, AtomTag, Compute, Origin};
//!
//! // Create a validated atom tag.
//! let tag = AtomTag::try_from("my-atom").unwrap();
//!
//! // Create an AtomId with a Git origin.
//! let repo = gix::open(".").unwrap();
//! let commit = repo
//!     .rev_parse_single("HEAD")
//!     .map(|s| repo.find_commit(s))
//!     .unwrap()
//!     .unwrap();
//!
//! let id = AtomId::construct(&commit, tag).unwrap();
//!
//! // Get the hash for disambiguated identification.
//! let hash = id.compute_hash();
//! println!("Atom hash: {}", hash);
//! ```

use std::borrow::Borrow;
use std::ffi::OsStr;
use std::fmt::{self, Display};
use std::ops::Deref;
use std::str::FromStr;

use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;
use unic_ucd_category::GeneralCategory;

#[cfg(test)]
mod tests;

/// The maximum allowed length of an atom identifier in bytes.
const ID_MAX: usize = 128;

/// A constant representing the root tag identifier.
pub(crate) const ROOT_TAG: &str = "__ROOT";

/// A type alias for `AtomTag` used in contexts requiring a validated identifier.
pub type Name = AtomTag;

/// A validated string suitable for use as an atom's `tag`.
///
/// `AtomTag` ensures that the identifier conforms to specific validation rules,
/// providing a safe and descriptive label for an atom within its origin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct AtomTag(String);

/// A struct that couples an atom's tag to its origin.
///
/// `AtomId` represents an unambiguous identifier, combining a human-readable
/// Unicode tag with a root field that varies by store implementation (e.g.,
/// the oldest commit in a Git repository).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AtomId<R> {
    origin: R,
    tag: AtomTag,
}

/// Represents the BLAKE3 hash of an `AtomId`.
///
/// This struct holds a 32-byte BLAKE3 hash, serving as a cryptographically
/// secure and globally unique identifier for an atom.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdHash<'id, T> {
    /// The 32-byte BLAKE3 hash value.
    hash: [u8; 32],
    /// A reference to the `AtomId` that was hashed.
    id: &'id AtomId<T>,
}

/// An enumeration of errors that can occur during atom tag validation.
///
/// These errors indicate failures in creating or parsing atom identifiers,
/// ensuring they adhere to the required format for secure identification.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum Error {
    /// The atom identifier is empty.
    #[error("An Atom id cannot be empty")]
    Empty,
    /// The atom identifier contains invalid characters.
    #[error("The Atom id contains invalid characters: '{0}'")]
    InvalidCharacters(String),
    /// The atom identifier starts with an invalid character.
    #[error("An Atom id cannot start with: '{0}'")]
    InvalidStart(char),
    /// The atom identifier contains invalid Unicode.
    #[error("An Atom id must be valid unicode")]
    InvalidUnicode,
    /// The atom identifier exceeds the maximum allowed length.
    #[error("An Atom id cannot be more than {} bytes", ID_MAX)]
    TooLong,
}

/// A trait for computing the BLAKE3 hash of an `AtomId`.
///
/// This trait provides a method to compute a cryptographically secure hash
/// that can serve as a unique identifier for an atom in storage backends.
pub trait Compute<'id, T>: Borrow<[u8]> {
    /// Computes the BLAKE3 hash of this `AtomId`.
    ///
    /// The hash is keyed with the atom's root value, ensuring that atoms with
    /// the same tag but different origins produce distinct hashes.
    ///
    /// # Returns
    ///
    /// An `IdHash` containing the 32-byte BLAKE3 hash and a reference to the
    /// original `AtomId`.
    fn compute_hash(&'id self) -> IdHash<'id, T>;
}

/// A trait for constructing new instances of an `AtomId`.
///
/// This trait defines how to calculate the `root` field for an `AtomId`.
pub trait Origin<R> {
    /// The error type returned by the `calculate_origin` method.
    type Error;

    /// Calculates the root field for the `AtomId`.
    ///
    /// # Errors
    ///
    /// This function will return an error if the calculation fails.
    fn calculate_origin(&self) -> Result<R, Self::Error>;
}

impl<R> AtomId<R> {
    /// Returns a reference to the atom's Unicode identifier.
    pub fn tag(&self) -> &AtomTag {
        &self.tag
    }
}

impl<R> AtomId<R>
where
    for<'id> AtomId<R>: Compute<'id, R>,
{
    /// Computes an atom's origin and constructs its ID.
    ///
    /// This method takes a source `src` that implements the `Origin` trait.
    ///
    /// # Errors
    ///
    /// This function will return an error if the call to `calculate_origin` fails.
    pub fn construct<T>(src: &T, tag: AtomTag) -> Result<Self, T::Error>
    where
        T: Origin<R>,
    {
        let origin = src.calculate_origin()?;
        Ok(AtomId { origin, tag })
    }

    /// Returns the root field, which serves as a derived key for the BLAKE3 hash.
    pub fn root(&self) -> &R {
        &self.origin
    }
}

impl AtomTag {
    /// Returns a special-purpose `AtomTag` for the repository root commit.
    pub(crate) fn root_tag() -> AtomTag {
        AtomTag(ROOT_TAG.into())
    }

    /// Checks if the tag is the root tag.
    pub(crate) fn is_root(&self) -> bool {
        self == &AtomTag::root_tag()
    }

    /// Validates that a character is a valid starting character.
    fn validate_start(c: char) -> Result<(), Error> {
        if AtomTag::is_invalid_start(c) {
            return Err(Error::InvalidStart(c));
        }
        Ok(())
    }

    /// Validates the entire string as a valid `AtomTag`.
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

    /// Checks if a character is an invalid starting character.
    pub(super) fn is_invalid_start(c: char) -> bool {
        matches!(
            GeneralCategory::of(c),
            GeneralCategory::DecimalNumber | GeneralCategory::LetterNumber
        ) || c == '_'
            || c == '-'
            || !AtomTag::is_valid_char(c)
    }

    /// Checks if a character is valid for use in an `AtomTag`.
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

impl<T> Borrow<[u8]> for AtomId<T> {
    fn borrow(&self) -> &[u8] {
        self.tag.as_bytes()
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

impl<R> Serialize for AtomId<R> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.tag.serialize(serializer)
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

impl TryFrom<String> for AtomTag {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        AtomTag::validate(&s)?;
        Ok(AtomTag(s))
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

impl<T> Deref for IdHash<'_, T> {
    type Target = [u8; 32];

    fn deref(&self) -> &Self::Target {
        &self.hash
    }
}
