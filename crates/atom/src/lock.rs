#[cfg(test)]
mod test;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use bstr::ByteSlice;
#[cfg(feature = "git")]
use gix::refs::PartialName;
use gix_url::Url;
use semver::{Version, VersionReq};
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize};

use crate::id::Id;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Dependencies {
    bonds: HashMap<Id, Bonds>,
    pins: HashMap<String, Pins>,
    srcs: HashMap<String, Src>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum Bonds {
    Local(PathBuf),
    Remote(Bond),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Bond {
    version: VersionReq,
    #[serde(deserialize_with = "parse_url")]
    url: Url,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum Pins {
    Indirect(FromPin),
    Direct(Pin),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FromPin {
    from: Id,
    get: Option<String>,
    entry: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    #[serde(deserialize_with = "parse_url")]
    url: Url,
    entry: Option<PathBuf>,
    #[cfg(feature = "git")]
    r#ref: Option<PartialName>,
    version: Option<Version>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Src {
    #[serde(deserialize_with = "parse_url")]
    url: Url,
    version: Option<Version>,
}

fn parse_url<'de, D>(deserializer: D) -> Result<Url, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Cow<'de, str> = Deserialize::deserialize(deserializer)?;
    let bytes = s.as_bytes().as_bstr();
    Url::from_bytes(bytes).map_err(D::Error::custom)
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Lock {
    version: String,
    eval: Option<Vec<LockedDeps>>,
    build: Option<Vec<LockedSrc>>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedSrc {
    name: String,
    #[serde(deserialize_with = "parse_url")]
    url: Url,
    checksum: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum LockedDeps {
    #[serde(rename = "atom")]
    Atom(LockedAtom),
    #[serde(rename = "pin")]
    Pin(LockedPinHttp),
    #[serde(rename = "pin+tar")]
    Tar(LockedPinHttp),
    #[serde(rename = "pin+git")]
    Git(LockedPinGit),
    #[serde(rename = "pin+from")]
    From(LockedPinFrom),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedAtom {
    id: Id,
    version: Version,
    path: Option<PathBuf>,
    #[cfg(feature = "git")]
    rev: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedPinHttp {
    name: String,
    #[serde(deserialize_with = "parse_url")]
    url: Url,
    checksum: String,
    entry: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedPinFrom {
    name: String,
    from: Id,
    get: Option<String>,
    entry: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedPinGit {
    name: String,
    #[serde(deserialize_with = "parse_url")]
    url: Url,
    #[cfg(feature = "git")]
    rev: String,
    entry: Option<PathBuf>,
}
