#[cfg(feature = "git")]
mod git;

use std::ffi::OsStr;
use std::fmt::Display;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Result;
use atom::uri::{AliasedUrl, Uri};
use clap::Parser;

#[derive(Debug, Clone)]
enum Ref {
    Atom(Uri),
    Pin(AliasedUrl),
}

#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
pub struct Args {
    /// The atom uri or URL to add as a dependency
    uri: Ref,
    /// The path to the atom to modify
    #[clap(default_value = ".")]
    path: PathBuf,
    #[command(flatten)]
    store: StoreArgs,
}

#[derive(Parser, Debug)]
struct StoreArgs {
    #[command(flatten)]
    #[cfg(feature = "git")]
    git: git::GitArgs,
}

pub(super) fn run(args: Args) -> Result<()> {
    use toml_edit::DocumentMut;

    let path = if args.path.file_name() == Some(OsStr::new("atom.toml")) {
        args.path
    } else {
        args.path.join("atom.toml")
    };
    let toml_str = fs::read_to_string(path)?;
    let mut _doc = toml_str.parse::<DocumentMut>()?;
    Ok(())
}

impl Display for Ref {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ref::Atom(uri) => uri.fmt(f),
            Ref::Pin(url) => url.fmt(f),
        }
    }
}

impl FromStr for Ref {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        if let Ok(uri) = s.parse::<Uri>() {
            return Ok(Ref::Atom(uri));
        }
        if let Ok(url) = s.parse::<AliasedUrl>() {
            return Ok(Ref::Pin(url));
        }
        Err(format!("Failed to parse '{}' to uri", s))
    }
}
