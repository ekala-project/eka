//! # Atom Identification Constructs
//!
//! This module contains the foundational types and logic for working with Atom
//! identifiers. Atom IDs are a crucial component for unambiguously keeping track
//! of Atoms from various sources without risk of collision or ambiguity.
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
/// A vetted String suitable for an atom's `id` field
pub struct Id(String);

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

pub trait ComputeHash<'id, T>: Borrow<[u8]> {
    fn compute_hash(&'id self) -> AtomHash<'id, T>;
}

/// This trait must be implemented to construct new instances of an an [`AtomId`].
/// It tells the [`AtomId::compute`] constructor how to calculate the value for
/// its `root` field.
pub trait CalculateRoot<R> {
    /// The error type returned by the [`CalculateRoot::calculate_root`] method.
    type Error;
    /// The method used the calculate the root field for the [`AtomId`].
    ///
    /// # Errors
    ///
    /// This function will return an error if the calculation fails or is impossible.
    fn calculate_root(&self) -> Result<R, Self::Error>;
}

/// The type representing all the components necessary to serve as
/// an unambiguous identifier. Atoms consist of a human-readable
/// Unicode identifier, as well as a root field, which varies for
/// each store implementation. For example, Git uses the oldest
/// commit in a repositories history.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AtomId<R> {
    root: R,
    id: Id,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AtomHash<'id, T> {
    hash: [u8; 32],
    id: &'id AtomId<T>,
}

impl<R> Serialize for AtomId<R> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize only the `id` field as a string
        self.id.serialize(serializer)
    }
}

impl<T> Deref for AtomHash<'_, T> {
    type Target = [u8; 32];

    fn deref(&self) -> &Self::Target {
        &self.hash
    }
}

impl<'id, R: AsRef<[u8]>> ComputeHash<'id, R> for AtomId<R> {
    fn compute_hash(&'id self) -> AtomHash<'id, R> {
        use blake3::Hasher;

        let key = blake3::derive_key("AtomId", self.root.as_ref());

        let mut hasher = Hasher::new_keyed(&key);
        hasher.update(self.id.as_bytes());
        AtomHash {
            hash: *hasher.finalize().as_bytes(),
            id: self,
        }
    }
}

impl<T> Borrow<[u8]> for AtomId<T> {
    fn borrow(&self) -> &[u8] {
        self.id.as_bytes()
    }
}

impl<R> AtomId<R>
where
    for<'id> AtomId<R>: ComputeHash<'id, R>,
{
    /// Compute and construct an Atom's ID. This method takes a `src`
    /// type which must implement a the [`CalculateRoot`] struct.
    ///
    /// # Errors
    ///
    /// This function will return an error if the call to
    /// [`CalculateRoot::calculate_root`] fails.
    pub fn compute<T>(src: &T, id: Id) -> Result<Self, T::Error>
    where
        T: CalculateRoot<R>,
    {
        let root = src.calculate_root()?;
        Ok(AtomId { root, id })
    }

    /// The root field, which serves as a derived key for the blake-3 hash used to
    /// identify the Atom in backend implementations.
    pub fn root(&self) -> &R {
        &self.root
    }
}

impl Id {
    fn validate_start(c: char) -> Result<(), Error> {
        if Id::is_invalid_start(c) {
            return Err(Error::InvalidStart(c));
        }
        Ok(())
    }

    pub(super) fn validate(s: &str) -> Result<(), Error> {
        if s.len() > ID_MAX {
            return Err(Error::TooLong);
        }

        match s.chars().next().map(Id::validate_start) {
            Some(Ok(())) => (),
            Some(Err(e)) => return Err(e),
            None => return Err(Error::Empty),
        }

        let invalid_chars: String = s.chars().filter(|&c| !Id::is_valid_char(c)).collect();

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
            || !Id::is_valid_char(c)
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

impl Deref for Id {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl FromStr for Id {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Id::validate(s)?;
        Ok(Id(s.to_string()))
    }
}

impl TryFrom<String> for Id {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Id::validate(&s)?;
        Ok(Id(s))
    }
}

impl TryFrom<&OsStr> for Id {
    type Error = Error;

    fn try_from(s: &OsStr) -> Result<Self, Self::Error> {
        let s = s.to_str().ok_or(Error::InvalidUnicode)?;
        Id::from_str(s)
    }
}

impl TryFrom<&str> for Id {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Id::from_str(s)
    }
}

use std::fmt::Display;

impl<R> AtomId<R> {
    /// Return a reference to the Atom's Unicode identifier.
    pub fn id(&self) -> &Id {
        &self.id
    }
}

impl<R> Display for AtomId<R>
where
    for<'id> AtomId<R>: ComputeHash<'id, R>,
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

impl<'a, R> Display for AtomHash<'a, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = base32::encode(crate::BASE32, &self.hash);
        if let Some(max_width) = f.precision() {
            write!(f, "{s:.max_width$}")
        } else {
            f.write_str(&s)
        }
    }
}
