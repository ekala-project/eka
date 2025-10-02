mod git;

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use atom::Lockfile;
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
    /// The atom uri or URL to add as a dependency. The TOML key inserted into the dependency
    /// `gh:owner/repo::my-atom`, `https://example.com/repo`.
    uri: UriOrUrl,
    /// The TOML key inserted into the dependency, serving as the name of the dependency in the
    /// source. Useful for avoiding conflicts (e.g. two different atoms with the same tag).
    #[clap(long, short)]
    key: Option<Name>,
    #[command(flatten)]
    store: StoreArgs,
}

#[derive(Parser, Debug)]
struct StoreArgs {
    #[command(flatten)]
    git: git::GitArgs,
}

pub(super) fn run(args: Args) -> Result<()> {
    let path = if args.path.file_name() == Some(OsStr::new(atom::MANIFEST_NAME.as_str())) {
        &args.path
    } else {
        &args.path.join(atom::MANIFEST_NAME.as_str())
    };
    let lock_path = path.with_file_name(atom::LOCK_NAME.as_str());
    let toml_str = fs::read_to_string(path).inspect_err(|_| {
        tracing::error!(message = "No atom exists", path = %path.display());
    })?;
    let (mut doc, manifest) = TypedDocument::new(&toml_str)?;

    let mut lock = if let Ok(lock_str) = fs::read_to_string(&lock_path) {
        toml_edit::de::from_str(&lock_str)?
    } else {
        Lockfile::default()
    };
    let owned_path = path.to_owned();
    lock.sanitize(&manifest);

    git::run(&mut doc, &mut lock, args)?;

    // create tmpfile for atomic writes
    let dir = owned_path.parent().ok_or(anyhow!(
        "the atom directory disappeared or is inaccessible: {}",
        &owned_path.display()
    ))?;
    let mut tmp = NamedTempFile::with_prefix_in(format!(".{}", atom::MANIFEST_NAME.as_str()), dir)?;
    let mut tmp_lock =
        NamedTempFile::with_prefix_in(format!(".{}", atom::LOCK_NAME.as_str()), dir)?;
    tmp.write_all(doc.as_mut().to_string().as_bytes())?;
    tmp_lock.write_all(toml_edit::ser::to_string_pretty(&lock)?.as_bytes())?;
    tmp.persist(&owned_path)?;
    tmp_lock.persist(lock_path)?;

    Ok(())
}
