use std::path::PathBuf;

use atom::storage::{Init, LocalStoragePath};
use clap::Parser;

use crate::cli::store::Detected;

#[derive(Parser, Debug)]
#[command(next_help_heading = "Init Options")]
#[group(id = "init_args")]
pub struct Args {
    /// The directory to initialize; if it doesn't exist, it will be created
    ///
    /// note: this argument is ignored if the current directory is already inside a git repository
    #[clap(default_value = ".")]
    dir: PathBuf,
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
        /// Initialize the current directory as a git repository before creating the ekala manifest
        ///
        /// note: does nothing if the current directory is already inside a git repository
        #[arg(long)]
        pub(super) init_git: bool,
    }
}

pub(super) fn run(store: Detected, args: Args) -> anyhow::Result<()> {
    match store {
        Detected::Git(repo) => {
            use atom::storage::Init;
            let repo = repo.to_thread_local();
            let remote = repo.find_remote(args.git.remote.as_str());

            repo.ekala_init(None)?;
            if let Ok(remote) = remote {
                remote.ekala_init(None)?;
            } else {
                tracing::warn!(
                    remote = %args.git.remote,
                    suggestion = "if you would like to publish your atoms, you can attempt initalization again later with a functional remote",
                    "initializing remote did not suceed"
                );
            };
        },
        Detected::FileStorage(local) => {
            tracing::info!(storage_root = %local.as_ref().display(), "already initalized");
        },
        Detected::None => {
            if args.git.init_git {
                let repo = gix::init(args.dir)?;
                repo.ekala_init(None)?;
            } else {
                if args.dir.try_exists().is_err() {
                    std::fs::create_dir_all(&args.dir)?;
                }
                LocalStoragePath::init(args.dir)?;
            }
            tracing::info!(message = "successfully initialized");
        },
    }
    Ok(())
}

#[test]
fn init_local() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;

    let args = Args {
        dir: tmp.as_ref().to_path_buf(),
        git: git::Args {
            remote: "origin".into(),
            init_git: false,
        },
    };
    run(Detected::None, args)?;
    assert!(
        tmp.as_ref()
            .join(atom::EKALA_MANIFEST_NAME.as_str())
            .try_exists()
            .is_ok()
    );
    Ok(())
}

#[test]
fn init_git_no_remote() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    unsafe {
        std::env::set_var("GIT_AUTHOR_NAME", "eka");
        std::env::set_var("GIT_AUTHOR_EMAIL", "eka@is-cool.com");
        std::env::set_var("GIT_COMMITTER_NAME", "eka");
        std::env::set_var("GIT_COMMITTER_EMAIL", "eka@is-cool.com");
    }

    let args = Args {
        dir: tmp.as_ref().to_path_buf(),
        git: git::Args {
            remote: "origin".into(),
            init_git: true,
        },
    };

    run(Detected::None, args)?;
    assert!(
        tmp.as_ref()
            .join(atom::EKALA_MANIFEST_NAME.as_str())
            .try_exists()
            .is_ok()
    );

    Ok(())
}
