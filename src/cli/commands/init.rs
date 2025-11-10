use atom::storage::LocalStoragePath;
use clap::Parser;

use crate::cli::store::Detected;

#[derive(Parser, Debug)]
#[command(next_help_heading = "Init Options")]
#[group(id = "init_args")]
pub struct Args {
    #[command(flatten)]
    git: git::Args,
}

mod git {
    use atom::storage::git;
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
    match store {
        Detected::Git(repo) => {
            use atom::storage::Init;
            let repo = repo.to_thread_local();
            let remote = repo.find_remote(args.git.remote.as_str())?;

            repo.ekala_init(None)?;
            if remote.ekala_init(None).is_err() {
                tracing::warn!(
                    remote = %args.git.remote,
                    suggestion = "if you would like to publish your atoms, you can attempt initalization again later with a functional remote",
                    "initializing repo did not suceed"
                );
            };
        },
        Detected::FileStorage(local) => {
            tracing::info!(storage_root = %local.as_ref().display(), "already initalized");
        },
        Detected::None => {
            LocalStoragePath::init(".")?;
            tracing::info!(message = "successfully initialized");
        },
    }
    Ok(())
}

#[test]
fn init_local() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    std::env::set_current_dir(tmp.as_ref())?;
    let args = Args {
        git: git::Args {
            remote: "origin".into(),
        },
    };
    run(Detected::None, args)?;
    assert!(
        tmp.as_ref()
            .join(atom::EKALA_MANIFEST_NAME.as_str())
            .exists()
    );
    Ok(())
}
