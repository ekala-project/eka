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
//! - A human-readable **label** (moniker) that is validated for descriptiveness and safety.
//! - A machine-readable **id** that combines the origin and label into a globally unique
//!   identifier, represented as a BLAKE3-derived cryptographic hash.
//!
//! These primitives support robust tooling for indexing, querying, and addressing
//! software packages with verifiable provenance.
//!
//! ## Key Concepts
//!
//! - **Atom Labels**: Unicode identifiers that label atoms within an origin. They are validated to
//!   ensure they contain only safe, descriptive characters.
//! - **Atom Ids**: A struct coupling a label to its origin, represented by a BLAKE3-derived hash.
//!   This provides a secure, collision-resistant, and stable identifier for the atom.
//! - **Identifiers**: Strict Unicode identifiers following UAX #31 without exceptions.
//! - **Tags**: Metadata tags allowing additional separators (`:` and `.`) for categorization.
//!
//! ## Label Validation Rules
//!
//! Atom labels must adhere to the following rules, which are based on the
//! [Unicode Standard Annex #31](https://unicode.org/reports/tr31/) for Unicode
//! Identifier and Pattern Syntax.
//!
//! - The input string is first normalized to NFKC (Normalization Form KC).
//! - The normalized string must not exceed 128 bytes in length.
//! - The normalized string must not be empty.
//! - The first character must be a character with the `XID_Start` property.
//! - All subsequent characters must have the `XID_Continue` property, with one exception: the
//!   hyphen (`-`) is allowed.
//!
//! In this sense atom labels are a superset of UAX #31 with an explicit exception for `-`. Tags are
//! also a superset of labels for purposes of meta-data and allow `:` and `.` as additional
//! separators. The explicit hierarchy is: Identifier ⊂ Label ⊂ Tag.
//!
//! ## Usage Example
//!
//! ```rust,no_run
//! use atom::storage::git::Root;
//! use atom::{AtomId, Compute, Label, Origin};
//!
//! // Create a validated atom label.
//! let label = Label::try_from("my-atom").unwrap();
//!
//! // Create an AtomId with a Git origin.
//! let repo = gix::open(".").unwrap();
//! let commit = repo
//!     .rev_parse_single("HEAD")
//!     .map(|s| repo.find_commit(s))
//!     .unwrap()
//!     .unwrap();
//!
//! let id = AtomId::construct(&commit, label).unwrap();
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

const ID_MAX: usize = 128;

//================================================================================================
// Types
//================================================================================================

/// Represents the BLAKE3 hash of an `AtomId`.
///
/// This struct holds a 32-byte BLAKE3 hash, serving as a cryptographically
/// secure and globally unique identifier for an atom.
#[derive(Copy, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AtomDigest([u8; 32]);

/// A struct that couples an atom's label to its origin.
///
/// `AtomId` represents an unambiguous identifier, combining a human-readable
/// Unicode label with a root field that varies by store implementation (e.g.,
/// the oldest commit in a Git repository).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AtomId<R> {
    origin: R,
    label: Label,
}

/// An enumeration of errors that can occur during atom label validation.
///
/// These errors indicate failures in creating or parsing atom identifiers,
/// ensuring they adhere to the required format for secure identification.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum Error {
    /// The atom identifier is empty.
    #[error("cannot be empty")]
    Empty,
    /// The identifier contains invalid characters.
    #[error("contains invalid characters: '{0}'")]
    InvalidCharacters(String),
    /// The identifier starts with an invalid character.
    #[error("cannot start with: '{0}'")]
    InvalidStart(char),
    /// The identifier contains invalid Unicode.
    #[error("must be valid unicode")]
    InvalidUnicode,
    /// The identifier exceeds the maximum allowed length.
    #[error("cannot be more than {} bytes", ID_MAX)]
    TooLong,
    /// Constructing atom digest from base32 string failed
    #[error("Invalid Base32 string")]
    InvalidBase32,
    /// Wrong byte array size for blake-3 sum
    #[error("Expected 32 bytes for BLAKE3 hash")]
    Blake3Bytes,
}

/// A validated string suitable for use as an atom's `label`.
///
/// `Label` ensures that the identifier conforms to specific validation rules,
/// providing a safe and descriptive label for an atom within its origin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct Identifier(String);

/// A type like `Label` implementing UAX #31 precisely (no exception for `-`)
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct Label(String);

/// A type alias for label in contexts where the term Label is confusing.
pub type Name = Label;

/// A type like `Label` but with exceptions for `:` and `.` characters for metadata tags
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct Tag(String);

//================================================================================================
// Traits
//================================================================================================

/// A trait for computing the BLAKE3 hash of an `AtomId`.
///
/// This trait provides a method to compute a cryptographically secure hash
/// that can serve as a unique identifier for an atom in storage backends.
pub trait Compute<'id, T>: Borrow<[u8]> {
    /// Computes the BLAKE3 hash of this `AtomId`.
    ///
    /// The hash is keyed with the atom's root value, ensuring that atoms with
    /// the same label but different origins produce distinct hashes.
    ///
    /// # Returns
    ///
    /// An `IdHash` containing the 32-byte BLAKE3 hash and a reference to the
    /// original `AtomId`.
    fn compute_hash(&'id self) -> AtomDigest;
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

/// A trait representing the unambiguous rules to validate and construct an identifier.
/// The default implementations are the rules used for atom labels described in the top-level module
/// documentation, but can be modified to allow for some flexibility, e.g. tags have identical
/// rules with the exception of allowing `:` as an additional allowed separator.
pub(crate) trait VerifiedName: private::VerifiedSeal + Deref {
    /// Validates that a character is a valid starting character.
    fn validate_start(c: char) -> Result<(), Error> {
        if !Self::is_valid_start(c) {
            return Err(Error::InvalidStart(c));
        }
        Ok(())
    }

    /// Constructor validating the entire string.
    fn validate(s: &str) -> Result<Self, Error> {
        use unicode_normalization::UnicodeNormalization;
        let normalized: String = s.nfkc().collect();

        if normalized.len() > ID_MAX {
            return Err(Error::TooLong);
        }

        match normalized.chars().next().map(Self::validate_start) {
            Some(Ok(())) => (),
            Some(Err(e)) => return Err(e),
            None => return Err(Error::Empty),
        }

        let invalid_chars: String = normalized
            .chars()
            .filter(|&c| !Self::is_valid_char(c))
            .collect();

        if !invalid_chars.is_empty() {
            return Err(Error::InvalidCharacters(invalid_chars));
        }

        Self::extra_validation(&normalized)?;

        Ok(private::VerifiedSeal::new_unverified(normalized))
    }

    /// Checks if a character is an invalid starting character.
    fn is_valid_start(c: char) -> bool {
        unicode_ident::is_xid_start(c)
    }

    /// Checks if a character is valid for use.
    fn is_valid_char(c: char) -> bool {
        unicode_ident::is_xid_continue(c)
    }

    /// Adds additional validation logic without overriding the default if required, does nothing by
    /// default.
    fn extra_validation(_s: &str) -> Result<(), Error> {
        Ok(())
    }
}

mod private {
    /// A private trait for constructing verified identifiers after validation.
    pub trait VerifiedSeal
    where
        Self: Sized,
    {
        /// used solely in the `VerifiedName` trait to construct the final value after it has been
        /// verified. This function should never be exposed publicly.
        fn new_unverified(s: String) -> Self;
    }
}

//================================================================================================
// Impls
//================================================================================================

impl<R> AtomId<R> {
    /// Returns a reference to the atom's Unicode identifier.
    pub fn label(&self) -> &Label {
        &self.label
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
    pub fn construct<T>(src: &T, label: Label) -> Result<Self, T::Error>
    where
        T: Origin<R>,
    {
        let origin = src.calculate_origin()?;
        Ok(AtomId { origin, label })
    }

    /// Returns the root field, which serves as a derived key for the BLAKE3 hash.
    pub fn root(&self) -> &R {
        &self.origin
    }
}

/// Unicode UAX #31 compliant identifier
impl VerifiedName for Identifier {}
impl private::VerifiedSeal for Identifier {
    fn new_unverified(s: String) -> Self {
        Self(s)
    }
}

impl VerifiedName for Label {
    fn is_valid_char(c: char) -> bool {
        Identifier::is_valid_char(c) || c == '-'
    }
}

impl private::VerifiedSeal for Label {
    fn new_unverified(s: String) -> Self {
        Self(s)
    }
}

impl VerifiedName for Tag {
    fn is_valid_char(c: char) -> bool {
        Label::is_valid_char(c) || c == '.' || c == ':'
    }

    fn extra_validation(s: &str) -> Result<(), Error> {
        if s.contains("..") {
            return Err(Error::InvalidCharacters("..".into()));
        }
        Ok(())
    }
}
impl private::VerifiedSeal for Tag {
    fn new_unverified(s: String) -> Self {
        Self(s)
    }
}

impl<T> Borrow<[u8]> for AtomId<T> {
    fn borrow(&self) -> &[u8] {
        self.label.as_bytes()
    }
}

impl<'id, R: AsRef<[u8]>> Compute<'id, R> for AtomId<R> {
    fn compute_hash(&'id self) -> AtomDigest {
        use blake3::Hasher;

        let key = blake3::derive_key("AtomId", self.origin.as_ref());

        let mut hasher = Hasher::new_keyed(&key);
        hasher.update(self.label.as_bytes());
        AtomDigest(*hasher.finalize().as_bytes())
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

impl Deref for Label {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Deref for Identifier {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Deref for Tag {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for Label {
    fn as_ref(&self) -> &str {
        (**self).as_str()
    }
}

impl Deref for AtomDigest {
    type Target = [u8; 32];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Display for AtomDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = base32::encode(crate::BASE32, &self.0);
        if let Some(max_width) = f.precision() {
            write!(f, "{s:.max_width$}")
        } else {
            f.write_str(&s)
        }
    }
}

impl FromStr for AtomDigest {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let array: [u8; 32] = base32::decode(crate::BASE32, s)
            .ok_or(Error::InvalidBase32)
            .and_then(|bytes| bytes.try_into().map_err(|_| Error::Blake3Bytes))?;
        Ok(AtomDigest(array))
    }
}

impl<'de> Deserialize<'de> for AtomDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let base32 = String::deserialize(deserializer)?;
        base32.parse().map_err(serde::de::Error::custom)
    }
}

impl Serialize for AtomDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let base32 = self.to_string();
        serializer.serialize_str(&base32)
    }
}

impl<R: AsRef<[u8]>> From<AtomId<R>> for AtomDigest {
    fn from(value: AtomId<R>) -> Self {
        value.compute_hash()
    }
}

impl FromStr for Label {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Label::validate(s)
    }
}

impl FromStr for Identifier {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Identifier::validate(s)
    }
}

impl FromStr for Tag {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Tag::validate(s)
    }
}

impl<R: AsRef<[u8]>> Serialize for AtomId<R> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hash = self.compute_hash().to_string();
        serializer.serialize_str(&hash)
    }
}

impl TryFrom<&OsStr> for Label {
    type Error = Error;

    fn try_from(s: &OsStr) -> Result<Self, Self::Error> {
        let s = s.to_str().ok_or(Error::InvalidUnicode)?;
        Label::from_str(s)
    }
}

impl TryFrom<&str> for Label {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Label::from_str(s)
    }
}

impl TryFrom<&str> for Identifier {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Identifier::from_str(s)
    }
}

impl TryFrom<String> for Label {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Label::validate(&s)
    }
}

impl TryFrom<String> for Identifier {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Identifier::validate(&s)
    }
}

impl TryFrom<String> for Tag {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Tag::validate(&s)
    }
}

impl TryFrom<&str> for Tag {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Tag::from_str(s)
    }
}

impl TryFrom<&OsStr> for Tag {
    type Error = Error;

    fn try_from(s: &OsStr) -> Result<Self, Self::Error> {
        let s = s.to_str().ok_or(Error::InvalidUnicode)?;
        Tag::from_str(s)
    }
}

//================================================================================================
// Tests
//================================================================================================

#[cfg(test)]
mod tests;
