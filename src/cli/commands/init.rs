use std::path::{Path, PathBuf};

use atom::package::EkalaManifest;
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

pub(super) fn run(store: Option<Detected>, args: Args) -> anyhow::Result<()> {
    #[allow(clippy::single_match)]
    match store {
        Some(Detected::Git(repo)) => {
            use atom::storage::Init;
            let repo = repo.to_thread_local();
            let remote = repo.find_remote(args.git.remote.as_str())?;

            repo.ekala_init(None)?;
            remote.ekala_init(None)?;
        },
        _ => {
            let dir = PathBuf::from(".");
            init_ekala(dir.join(atom::EKALA_MANIFEST_NAME.as_str()))?;
            tracing::info!(message = "successfully initialized");
        },
    }
    Ok(())
}

// FIXME: this should be run as a minimal implementation of the `Init` traits for repository-less
// sets
fn init_ekala<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
    if let Ok(content) = std::fs::read_to_string(&path) {
        toml_edit::de::from_str(&content)?
    } else {
        let manifest = EkalaManifest::new();
        std::fs::write(&path, toml_edit::ser::to_string_pretty(&manifest)?)?;
    };

    Ok(())
}
