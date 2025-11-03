//! This module defines the `resolve` subcommand.
//!
//! The `resolve` subcommand is responsible for resolving dependencies for a
//! given set of atoms and writing the results to a lock file.

use anyhow::Result;
use atom::ManifestWriter;
use atom::storage::LocalStorage;
use atom::uri::Uri;
use clap::Parser;

//================================================================================================
// Types
//================================================================================================

/// The `resolve` subcommand.
#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
pub struct Args {
    /// The URI of the local atom(s) to resolve dependencies for.
    uri: Vec<Uri>,
}

//================================================================================================
// Functions
//================================================================================================

/// The main entry point for the `resolve` subcommand.
pub(super) async fn run(storage: impl LocalStorage + 'static, args: Args) -> Result<()> {
    let ekala_root = storage.ekala_root_dir()?;

    let manifest = storage.ekala_manifest()?;

    async {
        for uri in args.uri {
            tracing::debug!(%uri, "attempting to resolve");
            if let Some(url) = uri.url() {
                if url.scheme == gix::url::Scheme::File && url.host().is_none() {
                    let path = &storage.normalize(url.path.to_string())?;
                    if manifest
                        .set()
                        .packages()
                        .as_ref()
                        .get_by_right(path)
                        .is_some()
                    {
                        // FIXME: create a global transport pool instead of storing them in the
                        // writer so we can do this asychronously
                        let writer = ManifestWriter::new(&storage, &ekala_root.join(path)).await?;
                        writer.write_atomic()?;
                        tracing::info!(%uri, "successfully resolved and wrote updates");
                    }
                } else {
                    tracing::warn!(%uri, "cannot lock external resources");
                    continue;
                }
            } else if let Some(path) = manifest.set().packages().as_ref().get_by_left(uri.label()) {
                let writer = ManifestWriter::new(&storage, &ekala_root.join(path)).await?;
                writer.write_atomic()?;
                tracing::info!(%uri, "successfully resolved and wrote updates");
            } else {
                tracing::warn!(%uri, "uri does not point at any locally declared atoms, skipping");
            }
        }
        Ok::<_, anyhow::Error>(())
    }
    .await?;
    Ok(())
}
