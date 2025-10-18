use std::path::Path;

use atom::manifest::EkalaManifest;
use clap::Parser;

use crate::cli::store::Detected;

#[derive(Parser, Debug)]
#[command(next_help_heading = "Init Options")]
#[group(id = "init_args")]
pub struct Args {
    /// The name of the project wide package set.
    ///
    /// This name will be used in the context string of the KDF function to generate an atom's
    /// blake3 cryptographic fingerprint. Therefore, changing the name in the future will have the
    /// result of changing all the atoms' proper identity in the set.
    #[clap(long, short)]
    name: Option<String>,
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

            let workdir = repo.workdir().ok_or(anyhow::anyhow!(
                "must be in a git work directory to create an ekala manifest in a git repository"
            ))?;

            let ekala = get_ekala(workdir.join(atom::EKALA_MANIFEST_NAME.as_str()), args.name)?;

            remote.ekala_init(ekala.set().name(), None)?
        },
        _ => {},
    }
    Ok(())
}

fn get_ekala<P: AsRef<Path>>(path: P, name: Option<String>) -> anyhow::Result<EkalaManifest> {
    use inquire::{Confirm, Text};

    let ekala: EkalaManifest = if let Ok(content) = std::fs::read_to_string(&path) {
        if let Some(name) = name {
            tracing::warn!(
                message =
                    "`--name` was passed, but an existing `{atom::EKALA_MANIFEST_NAME}` already exists, ignoring..",
                    %name

            );
        }
        toml_edit::de::from_str(&content)?
    } else {
        let root_name = if let Some(name) = name {
            name
        } else {
            let mut name: String;
            use gix::validate::reference;
            loop {
                name = Text::new("What would you like to name your project?")
                    .with_help_message(
                        "This will be the proper name of your atom set, affecting their \
                         cryptographic thumbprint.",
                    )
                    .prompt()?;

                if reference::name_partial(name.as_str().into())
                    .map_err(|e| {
                        tracing::warn!(message = "name is not valid in a git ref, try again", %name, error = %e);
                    })
                    .is_err()
                    {
                        continue;
                    }

                let confirmed = Confirm::new(&format!("Is '{}' correct?", name))
                    .with_default(true)
                    .prompt()?;

                if confirmed {
                    break;
                }
            }
            name
        };

        let manifest = EkalaManifest::new(root_name)?;
        std::fs::write(&path, toml_edit::ser::to_string_pretty(&manifest)?)?;
        manifest
    };

    Ok(ekala)
}
