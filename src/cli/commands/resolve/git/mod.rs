//! This module defines Git-specific arguments for the `resolve` subcommand.

use atom::store::git;
use clap::Parser;

/// Git-specific arguments for the `resolve` subcommand.
#[derive(Parser, Debug)]
#[command(next_help_heading = "Git Options")]
pub(super) struct GitArgs {
    /// The target remote to lock to.
    #[arg(long, short = 't', default_value_t = git::default_remote().to_owned(), name = "TARGET")]
    remote: String,
}
