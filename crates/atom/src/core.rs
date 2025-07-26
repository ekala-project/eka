use std::borrow::Cow;
use std::path::Path;

use semver::Version;
use serde::{Deserialize, Serialize};

use super::id::Id;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
/// Represents the deserialized form of an Atom, directly constructed from the TOML manifest.
///
/// This struct contains the basic metadata of an Atom but lacks the context-specific
/// [`crate::AtomId`], which must be constructed separately.
#[serde(deny_unknown_fields)]
pub struct Atom {
    /// The verified, human-readable Unicode identifier for the Atom.
    pub id: Id,

    /// The version of the Atom.
    pub version: Version,

    #[serde(skip_serializing_if = "Option::is_none")]
    /// An optional description of the Atom.
    pub description: Option<String>,
}

#[derive(Debug)]
pub(crate) struct AtomPaths<P>
where
    P: AsRef<Path>,
{
    spec: P,
    content: P,
    lock: P,
}

const LOCK: &str = "lock";
use std::path::PathBuf;
impl AtomPaths<PathBuf> {
    pub(crate) fn new<P: AsRef<Path>>(path: P) -> Self {
        let name = path.as_ref().with_extension("");
        let name = name
            .file_name()
            .unwrap_or(path.as_ref().as_os_str())
            .to_string_lossy();

        let atom_name: Cow<str>;
        let content_name: Cow<str>;

        if name.ends_with('@') {
            atom_name = name;
            let mut name = atom_name.to_string();
            name.pop();
            content_name = name.into();
        } else {
            atom_name = format!("{name}@").into();
            content_name = name;
        };

        let content = path.as_ref().with_file_name(content_name.as_ref());
        AtomPaths {
            spec: path
                .as_ref()
                .with_file_name(atom_name.as_ref())
                .with_extension(crate::TOML),
            content: content.clone(),
            lock: content.with_extension(LOCK),
        }
    }

    pub fn lock(&self) -> &Path {
        self.lock.as_ref()
    }

    pub fn spec(&self) -> &Path {
        self.spec.as_ref()
    }

    pub fn content(&self) -> &Path {
        self.content.as_ref()
    }
}
