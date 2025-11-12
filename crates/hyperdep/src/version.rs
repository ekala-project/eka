//! # Version Abstractions
//!
//! Generic version and version-range abstractions that work with `semver` today
//! and any future version scheme tomorrow.
//!
//! This module provides the foundation for representing version constraints and
//! requirements in a generic way, allowing dependency resolvers to work with
//! different version schemes uniformly.
//!
//! ## Features
//!
//! - `semver`: Enables semver-specific parsing and matching
//!
//! ## Example
//!
//! ```
//! use hyperdep::{Requirement, Version, VersionRange};
//!
//! // Create a requirement for a package
//! let req = Requirement {
//!     package: "tokio".to_string(),
//!     range: VersionRange::Exact("1.0.0".to_string()),
//! };
//!
//! // Check if a version satisfies the range
//! assert!(req.range.contains(&"1.0.0".to_string()));
//! ```

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// A version identifier that must be totally ordered.
///
/// This trait represents any type that can be used as a version identifier.
/// It requires the version to be comparable, cloneable, and debug-printable.
///
/// # Examples
///
/// ```
/// use hyperdep::Version;
///
/// // String versions
/// fn works_with_strings<T: Version>(version: T) {
///     // Can compare, clone, and debug print versions
/// }
///
/// // Custom version types
/// #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
/// struct MyVersion(u32);
/// ```
pub trait Version: Ord + Clone + fmt::Debug {}
impl<T: Ord + Clone + fmt::Debug> Version for T {}

/// A predicate for matching versions that can be cloned and boxed.
pub trait VersionPredicate<V>: fmt::Debug {
    /// Tests whether a version matches this predicate.
    fn matches(&self, v: &V) -> bool;
    /// Clones this predicate into a boxed trait object.
    fn clone_box(&self) -> Box<dyn VersionPredicate<V>>;
}

/// A predicate that matches versions based on a semver requirement.
#[cfg(feature = "semver")]
#[derive(Clone, Debug)]
pub struct SemverPredicate {
    req: semver::VersionReq,
}

#[cfg(feature = "semver")]
impl VersionPredicate<semver::Version> for SemverPredicate {
    fn matches(&self, v: &semver::Version) -> bool {
        self.req.matches(v)
    }

    fn clone_box(&self) -> Box<dyn VersionPredicate<semver::Version>> {
        Box::new(self.clone())
    }
}

/// A requirement specifying constraints on a single package.
///
/// A requirement consists of a package name and a version range that defines
/// which versions of the package are acceptable.
///
/// # Examples
///
/// ```
/// use hyperdep::{Requirement, VersionRange};
///
/// let req = Requirement {
///     package: "serde".to_string(),
///     range: VersionRange::Exact("1.0.0".to_string()),
/// };
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Requirement<V> {
    /// The name of the package this requirement applies to.
    pub package: String,
    /// The version range that defines acceptable versions.
    pub range: VersionRange<V>,
}

/// A range defining which versions are acceptable for a package.
///
/// Version ranges provide flexible ways to specify version constraints, from
/// accepting any version to custom logic defined by closures.
///
/// # Examples
///
/// ```
/// use hyperdep::VersionRange;
///
/// // Accept any version
/// let any = VersionRange::<String>::Any;
///
/// // Accept only exact version
/// let exact = VersionRange::Exact("1.0.0".to_string());
///
/// // Accept from a list of candidates
/// let candidates = VersionRange::Candidates(vec!["1.0.0".to_string(), "1.1.0".to_string()]);
/// ```
pub enum VersionRange<V> {
    /// Any version is acceptable.
    ///
    /// This variant matches all possible versions and is useful when there
    /// are no version constraints.
    Any,
    /// Accept only an exact version match.
    ///
    /// This is the most restrictive range, requiring an exact version match.
    Exact(V),
    /// Accept versions from a specific set of candidates.
    ///
    /// Useful for lock-files where only pre-approved versions should be used.
    Candidates(Vec<V>),
    /// Custom range defined by a predicate.
    ///
    /// The predicate must be pure and deterministic.
    Predicate(String, Box<dyn VersionPredicate<V>>),
}

// Manual implementations for VersionRange due to the Predicate variant

impl<V> Clone for VersionRange<V>
where
    V: Clone,
{
    fn clone(&self) -> Self {
        match self {
            VersionRange::Any => VersionRange::Any,
            VersionRange::Exact(v) => VersionRange::Exact(v.clone()),
            VersionRange::Candidates(v) => VersionRange::Candidates(v.clone()),
            VersionRange::Predicate(s, p) => VersionRange::Predicate(s.clone(), p.clone_box()),
        }
    }
}

impl<V> core::fmt::Debug for VersionRange<V>
where
    V: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VersionRange::Any => write!(f, "Any"),
            VersionRange::Exact(v) => write!(f, "Exact({:?})", v),
            VersionRange::Candidates(v) => write!(f, "Candidates({:?})", v),
            VersionRange::Predicate(s, _) => write!(f, "Predicate({}, <closure>)", s),
        }
    }
}

// Custom trait implementations for VersionRange to handle the Predicate variant
// containing closures that don't implement the standard derived traits

impl<V: PartialEq> PartialEq for VersionRange<V> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (VersionRange::Any, VersionRange::Any) => true,
            (VersionRange::Exact(a), VersionRange::Exact(b)) => a == b,
            (VersionRange::Candidates(a), VersionRange::Candidates(b)) => a == b,
            (VersionRange::Predicate(a, _), VersionRange::Predicate(b, _)) => a == b,
            _ => false,
        }
    }
}

impl<V: Eq> Eq for VersionRange<V> {}

impl<V: PartialOrd> PartialOrd for VersionRange<V> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        match (self, other) {
            (VersionRange::Any, VersionRange::Any) => Some(core::cmp::Ordering::Equal),
            (VersionRange::Exact(a), VersionRange::Exact(b)) => a.partial_cmp(b),
            (VersionRange::Candidates(a), VersionRange::Candidates(b)) => a.partial_cmp(b),
            (VersionRange::Predicate(a, _), VersionRange::Predicate(b, _)) => a.partial_cmp(b),
            // Different variants are not comparable
            _ => None,
        }
    }
}

impl<V: Ord> Ord for VersionRange<V> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        match (self, other) {
            (VersionRange::Any, VersionRange::Any) => core::cmp::Ordering::Equal,
            (VersionRange::Exact(a), VersionRange::Exact(b)) => a.cmp(b),
            (VersionRange::Candidates(a), VersionRange::Candidates(b)) => a.cmp(b),
            (VersionRange::Predicate(a, _), VersionRange::Predicate(b, _)) => a.cmp(b),
            // Order by variant
            (VersionRange::Any, _) => core::cmp::Ordering::Less,
            (_, VersionRange::Any) => core::cmp::Ordering::Greater,
            (VersionRange::Exact(_), _) => core::cmp::Ordering::Less,
            (_, VersionRange::Exact(_)) => core::cmp::Ordering::Greater,
            (VersionRange::Candidates(_), _) => core::cmp::Ordering::Less,
            (_, VersionRange::Candidates(_)) => core::cmp::Ordering::Greater,
            // Predicate comparison is handled above
        }
    }
}

impl<V: Version> VersionRange<V> {
    /// Tests whether a concrete version satisfies this range.
    ///
    /// # Arguments
    ///
    /// * `v` - The version to test against this range
    ///
    /// # Returns
    ///
    /// `true` if the version satisfies the range constraints, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// use hyperdep::VersionRange;
    ///
    /// let range = VersionRange::Exact("1.0.0".to_string());
    /// assert!(range.contains(&"1.0.0".to_string()));
    /// assert!(!range.contains(&"1.1.0".to_string()));
    ///
    /// let any = VersionRange::<String>::Any;
    /// assert!(any.contains(&"any.version".to_string()));
    /// ```
    pub fn contains(&self, v: &V) -> bool {
        match self {
            VersionRange::Any => true,
            VersionRange::Exact(e) => e == v,
            VersionRange::Candidates(cands) => cands.iter().any(|c| c == v),
            VersionRange::Predicate(_, p) => p.matches(v),
        }
    }
}

/* --------------------------------------------------------------------- */
/* Semver-specific helpers – only compiled when the `semver` feature is on */
/* --------------------------------------------------------------------- */

/// Semver-specific functionality for parsing and working with semantic versions.
///
/// This module provides convenient parsing of semver requirements from strings
/// and integration with the semver crate.
#[cfg(feature = "semver")]
mod semver_support {
    use alloc::string::ToString;

    use semver::{Version, VersionReq};

    use super::*;

    impl<V: AsRef<str>> From<V> for Requirement<Version> {
        /// Parses a semver requirement from a string.
        ///
        /// The string should be in the format "package_name version_requirement".
        /// If no version requirement is provided, defaults to "*" (any version).
        ///
        /// # Examples
        ///
        /// ```
        /// # #[cfg(feature = "semver")]
        /// # {
        /// use hyperdep::Requirement;
        ///
        /// let req: Requirement<semver::Version> = "tokio ^1.2.0".into();
        /// assert_eq!(req.package, "tokio");
        /// # }
        /// ```
        fn from(s: V) -> Self {
            let s = s.as_ref();
            // Split on first whitespace: "tokio ^1.2.0"
            let (pkg, req) = s.split_once(' ').unwrap_or((s, "*"));
            let range = if req == "*" {
                VersionRange::Any
            } else {
                let vreq = VersionReq::parse(req).expect("invalid semver requirement");
                VersionRange::Predicate(req.to_string(), Box::new(SemverPredicate { req: vreq }))
            };
            Requirement {
                package: pkg.to_string(),
                range,
            }
        }
    }

    /// Convenience function to create a semver requirement from a string.
    ///
    /// This is equivalent to using `From::from()` or `into()`.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "semver")]
    /// # {
    /// use hyperdep::req;
    ///
    /// let requirement = req("serde ^1.0");
    /// # }
    /// ```
    pub fn req(s: impl AsRef<str>) -> Requirement<Version> {
        s.into()
    }
}

#[cfg(feature = "semver")]
pub use semver_support::*;
