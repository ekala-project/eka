//! This module defines the `add` subcommand.
//!
//! The `add` subcommand is responsible for adding dependencies to an atom's
//! manifest file. It can add dependencies from an atom URI or a pinned URL.

use std::path::PathBuf;

use anyhow::Result;
use atom::id::Name;
use atom::manifest::deps::GitSpec;
use atom::uri::{AliasedUrl, Uri};
use clap::{Parser, Subcommand};

use crate::cli::store::Detected;

//================================================================================================
// Types
//================================================================================================

/// The `add` subcommand.
#[derive(Parser, Debug)]
#[command(
    arg_required_else_help = true,
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true,
    next_help_heading = "Add Options"
)]
pub struct Args {
    /// The path to the atom to modify.
    #[clap(long, short, default_value = ".", global = true)]
    path: PathBuf,
    /// The atom URI to add as a dependency.
    #[clap(required = true)]
    uri: Option<Uri>, /* a required optional is used so that the subcommand properly negates it
                       * without causing a parser failure */
    /// The name of the package set to add this source to inside the manifest. Defaults to the last
    /// path segment if not specified.
    set: Option<Name>,
    #[command(subcommand)]
    sub: Option<AddSubs>,
}

/// Arguments for adding a dependency via Nix's builtin fetcher API.
#[derive(Parser, Debug)]
#[command(arg_required_else_help = true, next_help_heading = "Nix Options")]
pub struct NixArgs {
    /// The URL to add as a dependency.
    ///
    /// By default, a call to `builtins.fetchurl` is used, unless one of the following flags
    /// is passed.
    ///
    /// For convenience, user defined aliases will be expanded just as they are with atom uris.
    url: AliasedUrl,
    /// The TOML key inserted into the manifest, serving as the name of the dependency in the
    /// source. Useful if the desired name differs from the default, which is the final path
    /// component of the URL.
    #[clap(long, short)]
    key: Option<Name>,
    /// Uses the `builtins.fetchGit` fetcher. If the url scheme is ssh, or the path ends in
    /// ".git", this flag is assumed and overrides the others.
    ///
    /// Accepts either a git refspec or a semantic version request. If neither is passed,
    /// the revision of the `HEAD` ref will be resolved.
    ///
    /// Version requests are resolved intelligently compared against the git tags in the repo which
    /// conform to semantic version constraints, returning the highest match of the users request.
    #[clap(long, conflicts_with_all = ["build", "tar"], default_missing_value = "HEAD")]
    git: Option<GitSpec>,
    /// Use the `builtins.fetchTarball` fetcher. If the url contains a `.tar` extension, this
    /// flag is assumed, but can be disabled with `--tar=false` to fetch with `builtins.fetchurl`.
    #[clap(long, conflicts_with_all = ["build", "git"])]
    tar: Option<bool>,
    /// Uses the special builtin `<nix/fetchurl.nix>` fetcher to defer fetching to buildtime. This
    /// is primarily useful for dependencies that don't require evaluation, but are strictly build
    /// inputs.
    ///
    /// This is preferable as build time fetches are parallelized and don't block the evaluator;
    /// improving overall performance.
    ///
    /// However, it can actually *harm* performance to fetch evaluation dependencies at build time,
    /// as the evaluator must block to perform the build so it can read the value from it.
    ///
    /// It's important, then, to use this with the proper intent, and understand the difference.
    #[clap(
        long,
        conflicts_with_all = ["git", "tar"],
        default_value_if("exec", "true", Some("true")),
        default_value_if("unpack", "true", Some("true"))
    )]
    build: bool,
    /// Implies the `--build` flag, and will pass `unpack = true` to `<nix/fetchurl>`. This
    /// will be assumed if the `--build` flag is passed, and the url path contains a `.tar`
    /// extension. You can disable this auto-detection behavior by passing `--unpack=false`.
    #[clap(long, requires = "build", conflicts_with = "exec")]
    unpack: Option<bool>,
}

#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
pub struct DirectArgs {
    #[command(subcommand)]
    sub: DirectSubs,
}

#[derive(Subcommand, Debug)]
enum AddSubs {
    /// Add dependencies directly leveraging a backend specific API.
    ///
    /// This command requires an additional subcommand to specify the backend.
    Direct(DirectArgs),
}
#[derive(Subcommand, Debug)]
enum DirectSubs {
    /// Add dependencies from a given URL to the manifest using the builtin Nix fetchers API
    /// directly.
    ///
    /// This command takes a URL and updates the manifest and lock with the result.
    Nix(NixArgs),
}

//================================================================================================
// Functions
//================================================================================================

/// The main entry point for the `add` subcommand.
pub(super) async fn run(store: Option<Detected>, args: Args) -> Result<()> {
    let repo = match store {
        Some(Detected::Git(repo)) => Some(repo),
        _ => None,
    };
    let mut writer = atom::ManifestWriter::new(repo, &args.path).await?;

    if let Some(AddSubs::Direct(DirectArgs {
        sub: DirectSubs::Nix(args),
    })) = args.sub
    {
        writer
            .add_url(
                args.url,
                args.key,
                args.git,
                args.tar,
                args.build,
                args.unpack,
            )
            .await?;
    } else {
        writer.add_uri(args.uri.unwrap(), args.set)?;
    }

    writer.write_atomic()?;

    Ok(())
}
