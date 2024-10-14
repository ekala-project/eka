use std::path::PathBuf;

use semver::Version;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
enum Src {
    Url(Url),
    Path(PathBuf),
}

/// atom dependencies
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Atoms {
    version: Version,
    src: Src,
}

/// legacy pins and buildtime srcs. We use a single type to
/// represent both as they share the same form.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Srcs {
    src: Src,
    #[cfg(feature = "git")]
    r#ref: gix::refs::PartialName,
}

#[allow(dead_code)]
impl Src {
    pub(crate) fn url(self) -> Option<Url> {
        match self {
            Src::Url(url) => Some(url),
            _ => None,
        }
    }

    pub(crate) fn path(self) -> Option<PathBuf> {
        match self {
            Src::Path(path) => Some(path),
            _ => None,
        }
    }
}
