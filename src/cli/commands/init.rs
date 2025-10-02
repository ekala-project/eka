use clap::Parser;

use crate::cli::store::Detected;

#[derive(Parser, Debug)]
#[group(id = "init_args")]
pub struct Args {
    #[command(flatten)]
    git: git::Args,
}

mod git {
    use atom::store::git;
    use clap::Parser;
    #[derive(Parser, Debug)]
    #[command(next_help_heading = "Git Options")]
    #[group(id = "git_args")]
    pub(super) struct Args {
        /// The target remote to initialize
        #[arg(long, short = 't', default_value_t = git::default_remote().to_owned(), name = "TARGET")]
        pub(super) remote: String,
    }
}

pub(super) fn run(store: Detected, args: Args) -> anyhow::Result<()> {
    #[allow(clippy::single_match)]
    match store {
        Detected::Git(repo) => {
            use atom::store::Init;
            let repo = repo.to_thread_local();
            let remote = repo.find_remote(args.git.remote.as_str())?;
            remote.ekala_init(None)?
        },
        _ => {},
    }
    Ok(())
}
