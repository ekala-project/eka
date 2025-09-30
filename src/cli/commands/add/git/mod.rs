use anyhow::{Result, anyhow};
use atom::id::Name;
use atom::lock::{AtomDep, AtomLocation, Dep};
use atom::manifest::deps::{AtomReq, TypedDocument};
use atom::store::{QueryVersion, git};
use atom::uri::{AliasedUrl, Uri, UriOrUrl};
use atom::{AtomId, Lockfile, Manifest};
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
    for uri in args.uri {
        match uri {
            (UriOrUrl::Atom(uri), key) => process_uri(uri, key, doc, lock)?,
            (UriOrUrl::Pin(aliased_url), _) => process_url(aliased_url, doc)?,
        }
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
        &VersionReq::parse("*")?
    };

    if let Some(url) = url {
        let atoms = url.get_atoms(None)?;
        let maybe_atom =
            <gix::Url as QueryVersion<_, _, _, _>>::process_highest_match(atoms.clone(), tag, req);
        let id = AtomId::construct(&atoms, tag.to_owned())?;
        if let Some((version, object)) = maybe_atom {
            let url_str = url.to_string();
            let ver_req = if let Some(v) = maybe_version {
                v
            } else {
                &VersionReq::parse(&version.to_string())?
            };
            let key = if let Some(key) = key {
                key
            } else {
                tag.to_owned()
            };
            let atom: AtomReq = AtomReq::new(
                ver_req.to_owned(),
                url_str.parse()?,
                (&key != tag).then(|| tag.to_owned()),
            );
            // TODO: handle locking
            let lock_entry: AtomDep = AtomDep {
                name: (&key != tag).then(|| key.to_owned()),
                tag: tag.to_owned(),
                id: id.into(),
                version,
                rev: object.into(),
                location: Some(AtomLocation::Url(url_str.parse()?)),
            };

            if lock
                .deps
                .as_mut()
                .insert(key.to_owned(), Dep::Atom(lock_entry))
                .is_some()
            {
                tracing::warn!("updating `{}`", key);
            }

            doc.write_atom_dep(key.as_str(), &atom)?;
        } else {
            return Err(anyhow!("No atom `{}` matching `{}` at `{}`", tag, req, url));
        };
    } else {
        // search locally for atom tag
        todo!()
    }

    Ok(())
}
fn process_url(_uri: AliasedUrl, _doc: &mut TypedDocument<Manifest>) -> Result<()> {
    Ok(())
}
