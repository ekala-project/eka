use atom::store::git;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(next_help_heading = "Git Options")]
pub(super) struct GitArgs {
    /// The target remote to derive the url for local atom refs
    #[arg(long, short = 't', default_value_t = git::default_remote().to_owned(), name = "TARGET", global = true)]
    pub(super) remote: String,
}
