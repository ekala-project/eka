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
            LocalStoragePath::init(".")?;
            tracing::info!(message = "successfully initialized");
        },
    }
    Ok(())
}

#[cfg(test)]
use std::time::Duration;

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
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        tmp.as_ref()
            .join(atom::EKALA_MANIFEST_NAME.as_str())
            .exists()
    );
    Ok(())
}

#[test]
fn init_git_no_remote() -> anyhow::Result<()> {
    use atom::storage;
    let tmp = tempfile::tempdir()?;
    std::thread::sleep(Duration::from_millis(50));
    std::env::set_current_dir(tmp.as_ref())?;
    std::thread::sleep(Duration::from_millis(50));
    gix::init(&tmp)?;
    std::thread::sleep(Duration::from_millis(50));
    let repo = storage::git::repo()?.expect("test repo not detected");

    let args = Args {
        git: git::Args {
            remote: "origin".into(),
        },
    };

    run(Detected::Git(repo), args)?;

    assert!(
        tmp.as_ref()
            .join(atom::EKALA_MANIFEST_NAME.as_str())
            .exists()
    );

    Ok(())
}
