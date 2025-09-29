#[cfg(feature = "git")]
mod git;

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use atom::Manifest;
use atom::id::Name;
use atom::manifest::deps::TypedDocument;
use atom::uri::UriOrUrl;
use clap::Parser;
use tempfile::NamedTempFile;

#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
pub struct Args {
    /// The path to the atom to modify
    #[clap(long, short, default_value = ".")]
    path: PathBuf,
    #[clap(value_parser = split_names)]
    /// The atom uri or URL to add as a dependency. The TOML key inserted into the dependency
    /// table can be renamed from the default by passing a name after a `,`, e.g.
    /// `gh:owner/repo::my-atom,key_name`, `https://example.com/repo,key_name`
    uri: Vec<(UriOrUrl, Option<Name>)>,
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
    let path = if args.path.file_name() == Some(OsStr::new(atom::MANIFEST_NAME.as_str())) {
        &args.path
    } else {
        &args.path.join(atom::MANIFEST_NAME.as_str())
    };
    let toml_str = fs::read_to_string(path).inspect_err(|_| {
        tracing::error!(message = "No atom exists", path = %path.display());
    })?;
    let mut doc: TypedDocument<Manifest> = TypedDocument::new(&toml_str)?;
    let owned_path = path.to_owned();
    #[cfg(feature = "git")]
    git::run(&mut doc, args)?;

    // create tmpfile for atomic writes
    let dir = owned_path.parent().ok_or(anyhow!(
        "the atom directory disappeared or is inaccessible: {}",
        &owned_path.display()
    ))?;
    let mut tmp = NamedTempFile::with_prefix_in(format!(".{}", atom::MANIFEST_NAME.as_str()), dir)?;
    tmp.write_all(doc.as_mut().to_string().as_bytes())?;
    tmp.persist(&owned_path)?;

    Ok(())
}

fn split_names(uri: &str) -> Result<(UriOrUrl, Option<Name>), atom::uri::UriError> {
    let (url, name) = if let Some((url, name)) = uri.rsplit_once(',') {
        (url, Some(name.parse()?))
    } else {
        (uri, None)
    };

    Ok((url.parse()?, name))
}
