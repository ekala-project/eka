mod git;

use std::path::PathBuf;

use anyhow::Result;
use atom::id::Name;
use atom::uri::UriOrUrl;
use clap::Parser;

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
    let mut writer = atom::ManifestWriter::new(&args.path)?;

    match args.uri {
        UriOrUrl::Atom(uri) => writer.add_uri(uri, args.key)?,
        UriOrUrl::Pin(_aliased_url) => todo!(),
    }

    writer.write_atomic()?;

    Ok(())
}
