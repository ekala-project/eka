use std::path::PathBuf;

use anyhow::Result;
use atom::{AtomTag, Manifest};
use clap::Parser;
use semver::Version;

#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
pub struct Args {
    /// The path to create the new atom in.
    path: PathBuf,
    /// The verbatim description of the atom.
    #[arg(short, long)]
    description: Option<String>,
    /// The version to initialize the atom at.
    #[arg(short = 'V', long, default_value = "0.1.0")]
    version: Version,
    /// The atom's `tag` (defaults the the last part of path)
    #[arg(short, long)]
    tag: Option<AtomTag>,
}

pub(super) fn run(args: Args) -> Result<()> {
    use std::ffi::OsStr;
    use std::fs;
    use std::io::Write;

    let tag: AtomTag = if let Some(tag) = args.tag {
        tag
    } else {
        args.path.file_name().unwrap_or(OsStr::new("")).try_into()?
    };
    let atom = Manifest::new(tag, args.version, args.description);
    let atom_str = toml_edit::ser::to_string_pretty(&atom)?;
    let atom_toml = args.path.join("atom.toml");

    fs::create_dir_all(&args.path)?;

    let mut dir = fs::read_dir(&args.path)?;

    if dir.next().is_some() {
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("Directory exists and is not empty: {:?}", args.path),
        ))?;
    }

    let mut toml_file = fs::File::create(atom_toml)?;
    toml_file.write_all(atom_str.as_bytes())?;

    Ok(())
}
