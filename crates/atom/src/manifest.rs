//! # Atom Manifest
//!
//! This module provides the core types for working with an Atom's manifest format.
//! The manifest is a TOML file that describes an atom's metadata and dependencies.
//!
//! ## Manifest Structure
//!
//! Every atom must have a manifest file named `atom.toml` that contains at minimum
//! a `[package]` section with the atom's label, version, and optional description.
//! Additional sections can specify package sets and dependencies.
//!
//! ## Package Sets and Mirrors
//!
//! The `[package.sets]` table defines named sources for atom dependencies. Each set
//! can be a single URL or an array of mirror URLs. The special value `"::"` represents
//! the local repository and enables efficient development workflows by allowing atoms
//! to reference each other without requiring `eka publish` after every change.
//!
//! This mirrors the URI format where `::<atom-name>` indicates a local atom from the
//! current repository (as opposed to remote atoms which would be prefixed with a URL or alias).
//!
//! ## Key Types
//!
//! - [`Manifest`] - The complete manifest structure, representing the `atom.toml` file.
//! - [`Atom`] - The core atom metadata (`label`, `version`, `description`, `sets`).
//! - [`Dependency`] - Atom and direct Nix dependencies (see [`deps`] module).
//! - [`AtomError`] - Errors that can occur during manifest processing.
//!
//! ## Example Manifest
//!
//! ```toml
//! [package]
//! label = "my-atom"
//! version = "1.0.0"
//! description = "A sample atom for demonstration"
//!
//! [package.sets]
//! company-atoms = "git@github.com:our-company/atoms"
//! local-atoms = "::"
//!
//! [deps.from.company-atoms]
//! other-atom = "^1.0.0"
//!
//! [deps.direct.nix]
//! external-lib.url = "https://example.com/lib.tar.gz"
//! ```
//!
//! ## Validation
//!
//! Manifests are strictly validated to ensure they contain all required fields
//! and have valid data. The `#[serde(deny_unknown_fields)]` attribute ensures
//! that only known fields are accepted, preventing typos and invalid configurations.
//!
//! ## Usage
//!
//! Manifests can be created programmatically or parsed from a string or file.
//!
//! ```rust,no_run
//! use std::str::FromStr;
//!
//! use atom::manifest::Manifest;
//! use atom::{Atom, Label};
//! use semver::Version;
//!
//! // Create a manifest programmatically.
//! let manifest = Manifest::new(
//!     Label::try_from("my-atom").unwrap(),
//!     Version::new(1, 0, 0),
//!     Some("My first atom".to_string()),
//! );
//!
//! // Parse a manifest from a string.
//! let manifest_str = r#"
//! [atom]
//! label = "parsed-atom"
//! version = "2.0.0"
//! "#;
//! let parsed = Manifest::from_str(manifest_str).unwrap();
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use gix::{Repository, ThreadSafeRepository};
use path_clean::PathClean;
use semver::Version;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use thiserror::Error;
use toml_edit::{DocumentMut, de};

use crate::id::Tag;
use crate::lock::BoxError;
use crate::manifest::deps::{Dependency, DocError, TypedDocument};
use crate::store::NormalizeStorePath;
use crate::{ATOM_MANIFEST_NAME, Atom, Label};

pub mod deps;
pub(crate) mod sets;

//================================================================================================
// Types
//================================================================================================

/// An error that can occur when parsing or handling an atom manifest.
#[derive(Error, Debug)]
pub enum AtomError {
    /// The manifest is missing the required `[atom]` table.
    #[error("Manifest is missing the `[package]` key")]
    Missing,
    /// One of the fields in the `[package]` table is missing or invalid.
    #[error(transparent)]
    InvalidAtom(#[from] de::Error),
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
    Doc(#[from] DocError),
    /// A generic boxed error
    #[error(transparent)]
    Generic(#[from] BoxError),
}

/// A strongly-typed representation of a source for an atom set.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SetMirror {
    /// Represents the local repository, allowing atoms to be resolved by path.
    #[serde(rename = "::")]
    Local,
    /// A URL pointing to a remote repository that serves as a source for an atom set.
    #[serde(
        serialize_with = "deps::serialize_url",
        deserialize_with = "deps::deserialize_url",
        untagged
    )]
    Url(gix::Url),
}

/// Represents the possible values for a named atom set in the manifest.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum AtomSet {
    /// A single source for an atom set.
    Singleton(SetMirror),
    /// A set of mirrors for an atom set.
    ///
    /// Since sets can be determined to be equivalent by their root hash, this allows a user to
    /// provide multiple sources for the same set. The resolver will check for equivalence at
    /// runtime by fetching the root commit from each URL. Operations like `publish` will
    /// error if inconsistent mirrors are detected.
    Mirrors(BTreeSet<SetMirror>),
}

/// Represents the structure of an `atom.toml` manifest file.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The required `[package]` table, containing core metadata.
    pub package: Atom,
    /// The dependencies of the atom.
    #[serde(default, skip_serializing_if = "Dependency::is_empty")]
    pub(crate) deps: Dependency,
}

/// A specialized result type for manifest operations.
pub type AtomResult<T> = Result<T, AtomError>;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct MetaData {
    tags: Option<BTreeSet<Tag>>,
}

/// The entrypoint for an ekala manifest describing a set of atoms.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EkalaManifest {
    set: EkalaSet,
    metadata: Option<MetaData>,
}

/// The section of the manifest describing the Ekala set of atoms.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EkalaSet {
    #[serde(default)]
    packages: AtomMap,
}

#[derive(Debug, PartialEq, Eq, Default)]
pub(crate) struct AtomMap(BTreeMap<Label, PathBuf>);

/// A writer to assist with writing into the Ekala manifest.
#[derive(Debug)]
pub struct EkalaManager {
    path: PathBuf,
    doc: TypedDocument<EkalaManifest>,
    repo: Option<Repository>,
    manifest: EkalaManifest,
}

//================================================================================================
// Impls
//================================================================================================

impl AsRef<BTreeMap<Label, PathBuf>> for AtomMap {
    fn as_ref(&self) -> &BTreeMap<Label, PathBuf> {
        &self.0
    }
}

impl AsMut<BTreeMap<Label, PathBuf>> for AtomMap {
    fn as_mut(&mut self) -> &mut BTreeMap<Label, PathBuf> {
        &mut self.0
    }
}

impl Manifest {
    /// Creates a new `Manifest` with the given label, version, and description.
    pub fn new(label: Label, version: Version, description: Option<String>) -> Self {
        Manifest {
            package: Atom {
                label,
                version,
                description,
                sets: HashMap::new(),
            },
            deps: Dependency::new(),
        }
    }

    /// Parses an [`Atom`] struct from the `[package]` table of a TOML document string,
    /// ignoring other tables and fields.
    ///
    /// # Errors
    ///
    /// This function will return an error if the content is invalid TOML,
    /// or if the `[package]` table is missing.
    pub(crate) fn get_atom(content: &str) -> AtomResult<Atom> {
        let doc = content.parse::<DocumentMut>()?;

        if let Some(v) = doc.get("package").map(ToString::to_string) {
            let atom = de::from_str::<Atom>(&v)?;
            Ok(atom)
        } else {
            Err(AtomError::Missing)
        }
    }

    pub(crate) fn get_atom_label<P: AsRef<Path>>(path: P) -> AtomResult<Label> {
        let content = std::fs::read_to_string(&path)?;
        let atom = Self::get_atom(&content)?;
        Ok(atom.label)
    }

    pub(crate) fn deps(&self) -> &Dependency {
        &self.deps
    }
}

impl std::fmt::Display for SetMirror {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SetMirror::Local => write!(f, "::"),
            SetMirror::Url(url) => write!(f, "{}", url),
        }
    }
}

impl FromStr for Manifest {
    type Err = de::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        de::from_str(s)
    }
}

impl TryFrom<PathBuf> for Manifest {
    type Error = AtomError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let content = std::fs::read_to_string(path)?;
        Ok(Manifest::from_str(&content)?)
    }
}

/// # AtomMap Deserialization: Enforcing Repository Path Invariants
///
/// This implementation provides a unique deserialization strategy for `AtomMap` that
/// conditionally enforces path normalization based on repository availability. This
/// behavior is crucial for maintaining the integrity of atom paths within the Ekala
/// ecosystem.
///
/// ## Purpose
///
/// The primary goal is to prevent atom paths from escaping the repository boundary.
/// Since `AtomMap` is constructed from the actual on-disk manifest during deserialization,
/// this provides an opportunity to validate and normalize paths early in the process,
/// failing fast if any path would violate the repository containment invariant.
///
/// ## Behavior
///
/// - **When inside a repository**: Paths are normalized using the repository's `normalize()`
///   method, which ensures all paths are relative to the repository root and contained within the
///   repository boundaries. If normalization fails (indicating a path outside the repository),
///   deserialization fails immediately.
///
/// - **When outside a repository**: Paths are left as-is, allowing normal operation in
///   non-repository contexts (e.g., testing, standalone usage).
///
/// ## Implementation Details
///
/// The implementation uses a global static reference to a lazily initialized repository
/// instance (`git::repo()`). This ensures:
/// - Efficient access: The repository is only discovered once per process
/// - Conditional behavior: Normalization only occurs when a repository is available
/// - Thread safety: Uses `OnceLock` for safe static initialization
///
/// ## Why This Matters
///
/// Atom paths must never escape the repository because they represent internal
/// references that are meaningless outside the repository context. By enforcing
/// this invariant during deserialization, we prevent invalid states that could
/// lead to data corruption, security issues, or inconsistent behavior.
///
/// ## Error Handling
///
/// If path normalization fails when a repository is present, deserialization
/// returns a custom error, preventing the creation of an invalid `AtomMap` instance.
/// This early failure ensures problems are caught during manifest loading rather
/// than later during atom resolution.
///
/// ## Additional Invariants
///
/// Beyond path containment, this implementation also enforces that no two atoms
/// share the same label. Since the map is keyed by `Label`, duplicate labels would
/// overwrite entries, losing atom identity. The implementation detects and rejects
/// such conflicts during deserialization, ensuring each atom maintains its distinct
/// identity.
impl<'de> Deserialize<'de> for AtomMap {
    /// Deserializes a list of paths into an `AtomMap`, enforcing repository path invariants.
    ///
    /// This function transforms a serialized list of paths into a map keyed by atom labels,
    /// acquired directly from each atom's manifest. This approach enables canonical addressing
    /// and efficient lookups while enforcing crucial invariants: path containment within the
    /// repository and uniqueness of atom labels.
    ///
    /// The deserialization process:
    /// 1. Deserializes the input as a `Vec<PathBuf>`
    /// 2. For each path, normalizes it relative to the repository root (when in a repo)
    /// 3. Reads the atom manifest to extract the canonical label
    /// 4. Ensures no duplicate labels exist
    /// 5. Constructs the final `BTreeMap<Label, PathBuf>`
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Path normalization fails (paths outside repository when in repo context)
    /// - Atom manifest cannot be read or parsed
    /// - Multiple atoms share the same label
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use crate::store::git;
        let repo = git::repo().ok().flatten().map(|r| r.to_thread_local());
        let entries: Vec<PathBuf> = Vec::deserialize(deserializer)?;
        let mut map = BTreeMap::new();

        let rel_to_root = repo
            .as_ref()
            .ok_or(crate::store::git::Error::NoWorkDir)
            .and_then(|r| r.rel_from_root(r.current_dir()));
        for path in entries {
            let normalized = if let Some(repo) = &repo {
                let rel_to_root = rel_to_root.as_ref().map_err(serde::de::Error::custom)?;
                let rel_path = rel_to_root.join(&path);
                let cwd = repo
                    .normalize(repo.current_dir())
                    .map_err(serde::de::Error::custom)?;
                let normal = repo
                    .normalize(&rel_path)
                    .map_err(serde::de::Error::custom)?;
                pathdiff::diff_paths(&normal, cwd)
                    .unwrap_or(rel_path)
                    .clean()
            } else {
                path.clean()
            };
            let label =
                Manifest::get_atom_label(normalized.join(crate::ATOM_MANIFEST_NAME.as_str()))
                    .map_err(serde::de::Error::custom)?;
            if let Some(path) = map.insert(label.to_owned(), normalized.to_owned()) {
                tracing::error!(
                    atoms.label = %label,
                    atoms.fst.path = %normalized.display(),
                    atoms.snd.path = %path.display(),
                    "two atoms share the same `label`"
                );
                return Err(serde::de::Error::custom(
                    "atoms must have unique labels to retain distinct identities",
                ));
            }
        }

        Ok(AtomMap(map))
    }
}

impl Serialize for AtomMap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let values: Vec<_> = self
            .as_ref()
            .values()
            .filter(|p| {
                p.join(ATOM_MANIFEST_NAME.as_str()).exists() || {
                    tracing::warn!(path = %p.display(), "atom does not exist, skipping serialization");
                    false
                }
            })
            .collect();
        values.serialize(serializer)
    }
}

impl EkalaManifest {
    /// Constructs a new Ekala manifest with the given set name
    pub fn new() -> Self {
        EkalaManifest {
            set: EkalaSet::new(),
            metadata: Some(MetaData::new()),
        }
    }

    /// Return a reference to the EkalaSet struct
    pub fn set(&self) -> &EkalaSet {
        &self.set
    }
}

impl Default for EkalaManifest {
    fn default() -> Self {
        Self::new()
    }
}

impl EkalaSet {
    fn new() -> Self {
        EkalaSet {
            packages: AtomMap::new(),
        }
    }

    fn _packages(&self) -> &AtomMap {
        &self.packages
    }
}

impl AtomMap {
    fn new() -> Self {
        AtomMap(BTreeMap::new())
    }
}

impl MetaData {
    fn new() -> Self {
        MetaData {
            tags: Some(BTreeSet::new()),
        }
    }
}

impl EkalaManager {
    /// Create a new manifest writer, traversing upward to locate the nearest ekala.toml if
    /// necessary.
    pub fn new(repo: Option<&ThreadSafeRepository>) -> Result<Self, AtomError> {
        let path = if let Some(repo) = repo {
            repo.work_dir()
                .map(|p| p.join(crate::EKALA_MANIFEST_NAME.as_str()))
                .ok_or(AtomError::EkalaManifest)?
        } else {
            let (_, manifest) = find_upwards(crate::EKALA_MANIFEST_NAME.as_str())?;
            manifest
        };

        let (doc, manifest) = {
            let content = std::fs::read_to_string(&path).inspect_err(|_| {
                tracing::error!(
                    suggestion = "did you run `eka init`?",
                    "{}",
                    AtomError::EkalaManifest
                )
            })?;
            TypedDocument::new(&content)?
        };

        Ok(EkalaManager {
            doc,
            path,
            manifest,
            repo: repo.map(|r| r.to_thread_local()),
        })
    }

    /// writes a new, minimal atom.toml to path, and updates the ekala.toml manifest
    pub fn new_atom_at_path(
        &mut self,
        label: Label,
        package_path: impl AsRef<Path>,
        version: Version,
        description: Option<String>,
    ) -> Result<(), crate::store::git::Error> {
        use std::fs;
        use std::io::Write;

        let mut tmp = NamedTempFile::with_prefix_in(format!(".new_atom-{}", label.as_str()), ".")?;

        let atom = Manifest::new(label.to_owned(), version, description);
        let atom_str = toml_edit::ser::to_string_pretty(&atom)?;
        let atom_toml = package_path.as_ref().join(ATOM_MANIFEST_NAME.as_str());

        tmp.write_all(atom_str.as_bytes())?;

        if package_path.as_ref().exists() {
            let mut dir = fs::read_dir(&package_path)?;

            if dir.next().is_some() {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "Directory exists and is not empty: {:?}",
                        package_path.as_ref().display()
                    ),
                ))?;
            }
            self.write_package(&package_path, label.to_owned())?;
        } else {
            fs::create_dir_all(&package_path)?;
            self.write_package(&package_path, label.to_owned())
                .inspect_err(|_| {
                    fs::remove_dir_all(&package_path).ok();
                })?;
        }
        tmp.persist(atom_toml)?;
        self.write_atomic()?;
        tracing::info!(
            message = "successfully added package to set",
            atom.label = %label,
            atom.path = %package_path.as_ref().display(),
            set = %self.path.display()
        );
        Ok(())
    }

    /// write a new package path into the packages list after verifying it is a valid atom
    fn write_package(
        &mut self,
        package_path: impl AsRef<Path>,
        label: Label,
    ) -> Result<(), crate::store::git::Error> {
        use toml_edit::{Array, Value};

        if let Some(path) = self.manifest.set.packages.as_ref().get(&label) {
            tracing::error!(
                suggestion = "rename one of them to maintain distinct identities",
                %label,
                manifest = %self.path.display(),
                atoms.existing.path = %path.display(),
                atoms.requested.path = %package_path.as_ref().display(),
                "atom with the given label already exists"
            );
            return Err(DocError::DuplicateAtoms.into());
        }

        use crate::store::NormalizeStorePath;
        let path = if let Some(repo) = &self.repo {
            repo.normalize(package_path)?
        } else {
            package_path.as_ref().into()
        };

        let doc = self.doc.as_mut();
        let set = doc
            .entry("set")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();

        let packages = set
            .entry("packages")
            .or_insert(toml_edit::value(Value::Array(Array::new())))
            .as_value_mut()
            .and_then(|v| v.as_array_mut())
            .unwrap();

        packages.push(path.display().to_string());
        packages.fmt();

        Ok(())
    }

    /// write the Ekala Manifest back to disk atomically
    fn write_atomic(&mut self) -> Result<(), DocError> {
        use std::io::Write;

        use tempfile::NamedTempFile;
        let dir = self.path.parent().ok_or(DocError::MissingEkala)?;
        let mut tmp = NamedTempFile::with_prefix_in(
            format!(".{}", crate::EKALA_MANIFEST_NAME.as_str()),
            dir,
        )?;
        tmp.write_all(self.doc.as_mut().to_string().as_bytes())?;
        tmp.persist(dir.join(crate::EKALA_MANIFEST_NAME.as_str()))?;
        Ok(())
    }
}

/// Returns the directory of the searched file and a path to the file itself as a tuple.
fn find_upwards(filename: &str) -> Result<(PathBuf, PathBuf), std::io::Error> {
    let start_dir = std::env::current_dir()?;

    for ancestor in start_dir.ancestors() {
        let file_path = ancestor.join(filename);
        if file_path.exists() {
            return Ok((ancestor.to_owned(), file_path));
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "could not locate ekala manifest",
    ))
}
