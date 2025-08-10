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
}

use std::path::PathBuf;
impl AtomPaths<PathBuf> {
    pub(crate) fn new<P: AsRef<Path>>(path: P) -> Self {
        let path = path.as_ref();
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();

        if name == crate::ATOM_MANIFEST.as_str() {
            AtomPaths {
                spec: path.into(),
                content: path.parent().unwrap_or(Path::new("")).into(),
            }
        } else {
            let spec = path.join(crate::ATOM_MANIFEST.as_str());
            AtomPaths {
                spec: spec.clone(),
                content: path.into(),
            }
        }
    }

    pub fn spec(&self) -> &Path {
        self.spec.as_ref()
    }

    pub fn content(&self) -> &Path {
        self.content.as_ref()
    }
}
