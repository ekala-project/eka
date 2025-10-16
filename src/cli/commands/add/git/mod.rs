//! This module defines Git-specific arguments for the `add` subcommand.

use atom::store::git;
use clap::Parser;

//================================================================================================
// Types
//================================================================================================

/// Git-specific arguments for the `add` subcommand.
#[derive(Parser, Debug)]
#[command(next_help_heading = "Git Options")]
pub(super) struct GitArgs {
    /// The target remote to derive the URL for local atom refs.
    #[arg(long, short = 't', default_value_t = git::default_remote().to_owned(), name = "TARGET", global = true)]
    pub(super) remote: String,
}
