//! This module defines the `publish --init` subcommand.
//!
//! The `publish --init` subcommand is responsible for initializing the
//! atom store in the underlying version control system.

use atom::store::Init;

use crate::cli::store::Detected;

//================================================================================================
// Functions
//================================================================================================

/// The main entry point for the `publish --init` subcommand.
pub(in super::super) fn run(store: Detected, args: super::StoreArgs) -> anyhow::Result<()> {
    #[allow(clippy::single_match)]
    match store {
        Detected::Git(repo) => {
            let repo = repo.to_thread_local();
            let remote = repo.find_remote(args.git.remote.as_str())?;
            remote.ekala_init(None)?
        },
        _ => {},
    }
    Ok(())
}
