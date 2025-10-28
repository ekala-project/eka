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
//! - [`Dependency`] - Atom and direct Nix dependencies.
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
//! use atom::{Atom, Label, Manifest};
//! use semver::Version;
//!
//! // Create a manifest programmatically.
//! let manifest = Manifest::new(Label::try_from("my-atom").unwrap(), Version::new(1, 0, 0));
//!
//! // Parse a manifest from a string.
//! let manifest_str = r#"
//! [package]
//! label = "parsed-atom"
//! version = "2.0.0"
//! "#;
//! let parsed = Manifest::from_str(manifest_str).unwrap();
//! ```
//!
//! Note: `Manifest` and `Atom` types are not publicly exposed in the current API.
//! Use the public exports from the crate root instead.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use gix::ThreadSafeRepository;
use id::{Tag, VerifiedName};
use lock::{AtomDep, Lockfile, SetDetails};
use package::AtomError;
use package::sets::{ResolvedSets, SetResolver};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, de};
use uri::{Uri, serde_gix_url};

use super::{DocError, GitDigest, TypedDocument, lock};
use crate::package::metadata::manifest::direct::DirectDeps;
use crate::{Atom, Label, id, package, uri};

pub(in crate::package) mod direct;

//================================================================================================
// Types
//================================================================================================

/// A strongly-typed representation of a source for an atom set.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SetMirror {
    /// Represents the local repository, allowing atoms to be resolved by path.
    #[serde(rename = "::")]
    Local,
    /// A URL pointing to a remote repository that serves as a source for an atom set.
    #[serde(with = "serde_gix_url", untagged)]
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
    package: Atom,
    /// The dependencies of the atom.
    #[serde(default, skip_serializing_if = "Dependency::is_empty")]
    deps: Dependency,
}

/// A specialized result type for manifest operations.
pub type AtomResult<T> = Result<T, AtomError>;

type AtomFrom = HashMap<Tag, HashMap<Label, VersionReq>>;

//================================================================================================
// Impls
//================================================================================================

impl Manifest {
    /// Creates a new `Manifest` with the given label, version, and description.
    pub fn new(label: Label, version: Version) -> Self {
        Manifest {
            package: Atom::new(label, version),
            deps: Dependency::new(),
        }
    }

    pub(in crate::package) fn deps(&self) -> &Dependency {
        &self.deps
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

    pub(super) fn get_atom_label<P: AsRef<Path>>(path: P) -> AtomResult<Label> {
        let content = std::fs::read_to_string(&path)?;
        let atom = Self::get_atom(&content)?;
        Ok(atom.take_label())
    }

    pub(in crate::package) fn package(&self) -> &Atom {
        &self.package
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

//================================================================================================
// Types
//================================================================================================

/// Represents a locked atom dependency, referencing a verifiable repository slice.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct AtomReq {
    /// The semantic version requirement for the atom (e.g., "^1.0.0").
    version: VersionReq,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
#[serde(deny_unknown_fields)]
/// The dependencies specified in the manifest
pub(in crate::package) struct Dependency {
    /// Specify atom dependencies from a specific set outlined in `[package.sets]`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    from: AtomFrom,
    /// Direct dependencies not in the atom format.
    #[serde(default, skip_serializing_if = "DirectDeps::is_empty")]
    direct: DirectDeps,
}

/// Represents the manner in which we resolve a rev for this git fetch
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum GitSpec {
    /// We will resolve the rev of the given ref.
    #[serde(rename = "ref")]
    Ref(String),
    /// We will resolve a version from the available tags resembling a semantic version.
    #[serde(rename = "version")]
    Version(VersionReq),
}

/// A writer for `atom.toml` manifests that ensures the `atom.lock` file is kept in sync.
///
/// # Example
///
/// ```rust,no_run
/// use std::path::Path;
///
/// use atom::ManifestWriter;
/// use atom::id::Tag;
/// use atom::uri::Uri;
///
/// async {
///     let mut writer = ManifestWriter::new(None, Path::new("/path/to/atom.toml"))
///         .await
///         .unwrap();
///     let uri = "my-atom@^1.0.0".parse::<Uri>().unwrap();
///     let key = "my-atom".parse::<Tag>().unwrap();
///     // Note: add_uri and write_atomic methods are not publicly exposed
///     // writer.add_uri(uri, Some(key)).unwrap();
///     // writer.write_atomic().unwrap();
/// };
/// ```
pub struct ManifestWriter {
    path: PathBuf,
    doc: TypedDocument<Manifest>,
    pub(in crate::package) lock: Lockfile,
    pub(in crate::package) resolved: ResolvedSets,
}

pub(in crate::package) struct AtomWriter {
    set_tag: Tag,
    atom_req: AtomReq,
    mirror: SetMirror,
}

//================================================================================================
// Traits
//================================================================================================

/// A trait for writing dependencies to a mutable TOML document representing an Atom manifest.
pub(in crate::package) trait WriteDeps<T: Serialize, K: VerifiedName> {
    /// The error type returned by the methods.
    type Error;

    /// Writes the dependency to the given TOML document.
    fn write_dep(&self, key: K, doc: &mut TypedDocument<T>) -> Result<(), Self::Error>;
}

//================================================================================================
// Impls
//================================================================================================

impl AsMut<AtomReq> for AtomReq {
    fn as_mut(&mut self) -> &mut AtomReq {
        self
    }
}

impl AsMut<Dependency> for Dependency {
    fn as_mut(&mut self) -> &mut Dependency {
        self
    }
}

impl<T: Serialize> AsMut<DocumentMut> for TypedDocument<T> {
    fn as_mut(&mut self) -> &mut DocumentMut {
        &mut self.inner
    }
}

impl FromStr for GitSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(req) = VersionReq::parse(s) {
            Ok(GitSpec::Version(req))
        } else {
            Ok(GitSpec::Ref(s.to_string()))
        }
    }
}

impl AtomReq {
    /// Creates a new `AtomReq` with the specified version requirement and location.
    pub fn new(version: VersionReq) -> Self {
        Self { version }
    }

    /// Returns a reference to the version requirement.
    pub fn version(&self) -> &VersionReq {
        &self.version
    }
}

impl Dependency {
    pub(super) fn new() -> Self {
        Dependency {
            from: HashMap::new(),
            direct: DirectDeps::new(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.from.is_empty() && self.direct.is_empty()
    }

    pub(crate) fn from(&self) -> &AtomFrom {
        &self.from
    }

    pub(crate) fn direct(&self) -> &DirectDeps {
        &self.direct
    }
}

impl ManifestWriter {
    pub(crate) const ATOM_BUG: &str = "bug, `AtomId` construction is infallible when derived \
                                       directly from a root and doesn't need to be calculated";
    pub(crate) const RESOLUTION_ERR_MSG: &str =
        "unlocked dependency could not be resolved remotely";
    pub(crate) const UPDATE_DEPENDENCY: &str =
        "updating out of date dependency in accordance with spec";

    /// Constructs a new `ManifestWriter`, ensuring that the manifest and lock file constraints
    /// are respected.
    pub async fn new(repo: Option<&ThreadSafeRepository>, path: &Path) -> Result<Self, AtomError> {
        use std::fs;
        let path = if path.file_name() == Some(OsStr::new(crate::ATOM_MANIFEST_NAME.as_str())) {
            path.into()
        } else {
            path.join(crate::ATOM_MANIFEST_NAME.as_str())
        };
        let lock_path = path.with_file_name(crate::LOCK_NAME.as_str());
        let toml_str = fs::read_to_string(&path).inspect_err(|_| {
            tracing::error!(message = "No atom exists", path = %path.display());
        })?;
        let (doc, manifest) = TypedDocument::new(&toml_str)?;
        let resolved_sets = SetResolver::new(repo, &manifest)?
            .get_and_check_sets()
            .await?;

        let lock = if let Ok(lock_str) = fs::read_to_string(&lock_path) {
            toml_edit::de::from_str(&lock_str)?
        } else {
            Lockfile::default()
        };
        let mut writer = ManifestWriter {
            doc,
            lock,
            path,
            resolved: resolved_sets,
        };
        writer.reconcile(manifest).await?;
        Ok(writer)
    }

    /// Runs the sanitization process, and then the synchronization process to ensure a fully
    /// consistent manifest and lock. This function is called in the `ManifestWriter` constructor
    /// to ensure that we are never operating on a stale manifest.
    async fn reconcile(&mut self, manifest: Manifest) -> Result<(), DocError> {
        self.set_sets();
        self.sanitize(&manifest);
        self.synchronize(manifest).await?;
        Ok(())
    }

    fn set_sets(&mut self) {
        self.lock.sets = self.resolved().details().to_owned();
    }

    pub(in crate::package) fn get_set_tag(
        &self,
        lock_entry: &AtomDep,
        uri: &Uri,
        set_tag_from_user: Option<Tag>,
    ) -> Tag {
        use package::resolve;

        self.resolved
            .details()
            .get(&lock_entry.set())
            .map(|s| s.tag.to_owned())
            .or(set_tag_from_user)
            .or_else(|| {
                if let Some(url) = uri.url() {
                    resolve::url_filename_as_tag(url).ok()
                } else if let Some(repo) = &self.resolved.repo {
                    repo.workdir()
                        .and_then(|p| p.canonicalize().ok())
                        .and_then(|p| p.file_stem().map(ToOwned::to_owned))
                        .and_then(|f| Tag::try_from(f.as_os_str()).ok())
                } else {
                    Tag::try_from("default").ok()
                }
            })
            .expect("bug; default tag should be infallible")
    }

    pub(in crate::package) fn update_lock_set(
        &mut self,
        set: GitDigest,
        mirror: SetMirror,
        tag: Tag,
    ) {
        use std::collections::btree_map::Entry;
        match self.lock.sets.entry(set) {
            Entry::Vacant(entry) => {
                entry.insert(SetDetails {
                    tag,
                    mirrors: BTreeSet::from([mirror]),
                });
            },
            Entry::Occupied(mut entry) => {
                entry.get_mut().mirrors.insert(mirror);
            },
        };
    }

    pub(in crate::package) fn resolved(&self) -> &ResolvedSets {
        &self.resolved
    }

    pub(in crate::package) fn doc_mut(&mut self) -> &mut TypedDocument<Manifest> {
        &mut self.doc
    }

    /// acquire a reference to the lockfile structure
    pub fn lock(&self) -> &Lockfile {
        &self.lock
    }

    /// acquire a reference to the manifest's path
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
}

impl WriteDeps<Manifest, Label> for AtomWriter {
    type Error = toml_edit::ser::Error;

    fn write_dep(&self, key: Label, doc: &mut TypedDocument<Manifest>) -> Result<(), Self::Error> {
        use toml_edit::{Array, Value};
        let doc = doc.as_mut();
        let mirror = self.mirror.to_string();

        let package = doc
            .entry("package")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();
        package.set_implicit(true);

        let sets = package
            .entry("sets")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();
        sets.set_implicit(true);

        let set = sets
            .entry(self.set_tag.as_str())
            .or_insert(toml_edit::value(Value::Array(Array::new())))
            .as_value_mut()
            .and_then(|v| v.as_array_mut())
            .unwrap();

        if !set.iter().any(|x| x.to_string().contains(&mirror)) {
            set.push(mirror);
            set.fmt();
        }

        let deps = doc
            .entry("deps")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();
        deps.set_implicit(true);

        let from = deps
            .entry("from")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();
        from.set_implicit(true);

        let set = from
            .entry(self.set_tag.as_str())
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();
        set.set_implicit(true);

        set[key.as_str()] = toml_edit::Item::Value(self.atom_req.version().to_string().into());

        doc.fmt();

        Ok(())
    }
}

impl AtomWriter {
    pub(in crate::package) fn new(set_tag: Tag, atom_req: AtomReq, mirror: SetMirror) -> Self {
        Self {
            set_tag,
            atom_req,
            mirror,
        }
    }
}

//================================================================================================
// Functions
//================================================================================================

/// A helper function for `serde(skip_serializing_if)` to omit `false` boolean values.
pub(crate) fn not(b: &bool) -> bool {
    !b
}
