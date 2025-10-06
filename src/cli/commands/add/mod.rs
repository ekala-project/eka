mod git;

use std::path::PathBuf;

use anyhow::Result;
use atom::id::Name;
use atom::uri::{AliasedUrl, Uri};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    arg_required_else_help = true,
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true,
    next_help_heading = "Add Options"
)]
pub struct Args {
    /// The path to the atom to modify
    #[clap(long, short, default_value = ".", global = true)]
    path: PathBuf,
    /// The atom URI to add as a dependency.
    #[clap(required = true)]
    uri: Option<Uri>,
    /// The TOML key inserted into the dependency, serving as the name of the dependency in the
    /// source. Useful for avoiding conflicts (e.g. two different atoms with the same tag).
    #[clap(long, short, global = true)]
    key: Option<Name>,
    #[command(flatten)]
    store: StoreArgs,
    #[command(subcommand)]
    pin: Option<PinCommand>,
}

#[derive(Subcommand, Debug)]
enum PinCommand {
    /// Add dependencies from a given URL to the manifest.
    ///
    /// This command takes a URL and updates the manifest and lock with the result.
    Pin(PinArgs),
}

#[derive(Parser, Debug)]
#[command(arg_required_else_help = true, next_help_heading = "Pin Options")]
pub struct PinArgs {
    /// The pinned URL to add as a dependency.
    url: AliasedUrl,
    /// Optional path to call `import` inside of the pinned resource. If not specified, the root of
    /// of the pin is assumed. The actual strategy for calling import depends on the libary being
    /// invoked. This flag is ignored for single file inputs (since their is no other path to
    /// import).
    import_path: Option<PathBuf>,
    /// Whether the pin should be imported as a Nix flake.
    flake: bool,
}

#[derive(Parser, Debug)]
struct StoreArgs {
    #[command(flatten)]
    git: git::GitArgs,
}

pub(super) async fn run(args: Args) -> Result<()> {
    let mut writer = atom::ManifestWriter::new(&args.path)?;

    if let Some(PinCommand::Pin(pin_args)) = args.pin {
        writer
            .add_url(pin_args.url, args.key, pin_args.import_path, pin_args.flake)
            .await?;
    } else {
        writer.add_uri(args.uri.unwrap(), args.key)?;
    }

    writer.write_atomic()?;

    Ok(())
}
