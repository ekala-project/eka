use anyhow::Result;
use atom::id::Name;
use atom::lock::Dep;
use atom::manifest::deps::{AtomReq, TypedDocument};
use atom::store::git;
use atom::uri::{AliasedUrl, Uri, UriOrUrl};
use atom::{Lockfile, Manifest};
use clap::Parser;
use semver::VersionReq;

use crate::cli::commands::add::Args;
#[derive(Parser, Debug)]
#[command(next_help_heading = "Git Options")]
pub(super) struct GitArgs {
    /// The target remote to derive the url for local atom refs
    #[arg(long, short = 't', default_value_t = git::default_remote().to_owned(), name = "TARGET")]
    pub(super) remote: String,
}

pub(super) fn run(
    doc: &mut TypedDocument<Manifest>,
    lock: &mut Lockfile,
    args: Args,
) -> Result<()> {
    match args.uri {
        UriOrUrl::Atom(uri) => process_uri(uri, args.key, doc, lock)?,
        UriOrUrl::Pin(aliased_url) => process_url(aliased_url, doc)?,
    }
    Ok(())
}

fn process_uri(
    uri: Uri,
    key: Option<Name>,
    doc: &mut TypedDocument<Manifest>,
    lock: &mut Lockfile,
) -> Result<()> {
    let tag = uri.tag();
    let maybe_version = uri.version();
    let url = uri.url();

    let req = if let Some(v) = maybe_version {
        v
    } else {
        &VersionReq::STAR
    };

    let key = if let Some(key) = key {
        key
    } else {
        tag.to_owned()
    };

    if let Some(url) = url {
        let mut atom: AtomReq = AtomReq::new(
            req.to_owned(),
            url.to_owned(),
            (&key != tag).then(|| tag.to_owned()),
        );
        let lock_entry = atom.resolve(&key)?;

        if maybe_version.is_none() {
            let version = VersionReq::parse(lock_entry.version.to_string().as_str())?;
            atom.set_version(version);
        };

        doc.write_atom_dep(key.as_str(), &atom)?;
        if lock
            .deps
            .as_mut()
            .insert(key.to_owned(), Dep::Atom(lock_entry))
            .is_some()
        {
            tracing::warn!("updating lock entry for `{}`", key);
        }
    } else {
        // search locally for atom tag
        todo!()
    }

    Ok(())
}
fn process_url(_uri: AliasedUrl, _doc: &mut TypedDocument<Manifest>) -> Result<()> {
    Ok(())
}
