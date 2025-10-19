use std::path::{Path, PathBuf};

use atom::Label;
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

pub(super) fn run(store: Option<Detected>, args: Args) -> anyhow::Result<()> {
    #[allow(clippy::single_match)]
    match store {
        Some(Detected::Git(repo)) => {
            use atom::store::Init;
            let repo = repo.to_thread_local();
            let remote = repo.find_remote(args.git.remote.as_str())?;

            let workdir = repo.workdir().ok_or(anyhow::anyhow!(
                "must be in a git work directory to create an ekala manifest in a git repository"
            ))?;

            let canon = workdir.canonicalize().ok();
            let default_prompt = canon
                .as_ref()
                .and_then(|p| p.file_stem())
                .and_then(|p| p.to_str());
            let ekala = init_ekala(
                workdir.join(atom::EKALA_MANIFEST_NAME.as_str()),
                args.name,
                default_prompt,
            )?;

            remote.ekala_init(ekala.set().label(), None)?;
        },
        _ => {
            let dir = PathBuf::from(".");
            let ekala = init_ekala(
                dir.join(atom::EKALA_MANIFEST_NAME.as_str()),
                args.name,
                None,
            )?;
            tracing::info!(message = "successfully initialized", project = %ekala.set().label());
        },
    }
    Ok(())
}

fn init_ekala<P: AsRef<Path>>(
    path: P,
    label: Option<String>,
    default: Option<&str>,
) -> anyhow::Result<EkalaManifest> {
    use inquire::{Confirm, Text};

    let ekala: EkalaManifest = if let Ok(content) = std::fs::read_to_string(&path) {
        if let Some(label) = label {
            tracing::warn!(
                message =
                    "`--name` was passed, but an existing `{atom::EKALA_MANIFEST_NAME}` already exists, ignoring..",
                    %label

            );
        }
        toml_edit::de::from_str(&content)?
    } else {
        let root_label = if let Some(label) = label {
            label
        } else {
            let mut label: String;
            loop {
                let prompt = Text::new("What would you like to name your project?");
                let prompt = if let Some(default) = default {
                    prompt.with_default(default)
                } else {
                    prompt
                };
                label = prompt
                    .with_help_message(
                        "This will be the proper name of your atom set, affecting their \
                         cryptographic thumbprint.",
                    )
                    .prompt()?;

                if Label::try_from(label.as_str())
                    .map_err(|e| {
                        tracing::warn!(message = "name is not a valid unicode identifier, try again", %label, error = %e);
                    })
                    .is_err()
                    {
                        continue;
                    }

                let confirmed = Confirm::new(&format!("Is '{}' correct?", label))
                    .with_default(true)
                    .prompt()?;

                if confirmed {
                    break;
                }
            }
            label
        };

        let manifest = EkalaManifest::new(root_label)?;
        std::fs::write(&path, toml_edit::ser::to_string_pretty(&manifest)?)?;
        manifest
    };

    Ok(ekala)
}
