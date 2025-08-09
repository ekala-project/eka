mod git;
use std::path::PathBuf;

use clap::Parser;

use crate::cli::store::Detected;

#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
pub struct Args {
    /// The path of the atom(s) to resolve dependencies for
    path: Vec<PathBuf>,
    #[command(flatten)]
    store: StoreArgs,
}

#[derive(Parser, Debug)]
struct StoreArgs {
    #[command(flatten)]
    #[cfg(feature = "git")]
    git: git::GitArgs,
}

pub(super) fn run(store: Detected, args: Args) {
    match store {
        #[cfg(feature = "git")]
        Detected::Git(repo) => {},
        _ => {},
    }
}
