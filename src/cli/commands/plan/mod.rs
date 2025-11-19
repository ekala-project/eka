use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use anyhow::anyhow;
use atom::Lockfile;
use atom::storage::git::NIX_IMPORT_FILE;
use atom::storage::{LocalStorage, QueryStore, QueryVersion, RemoteAtomCache, git};
use atom::uri::Uri;
use clap::Parser;
use config::CONFIG;
use semver::VersionReq;
use tempfile::TempDir;
use tokio::task::JoinSet;

#[derive(Parser, Debug)]
#[command(next_help_heading = "Plan Options", arg_required_else_help = true)]
#[group(id = "plan_args")]
pub struct Args {
    /// The atom uris to generate a plan for.
    #[clap(required = true)]
    uri: Vec<Uri>,
    /// Evaluate all the atoms in a shared evaluation context. The default is derived from the
    /// "eka.toml" via `plan.sharectx`, and is `false` if unset.
    #[clap(
        long,
        short = 'c',
        env = "EKA_PLAN_SHARECTX",
        default_value_t = CONFIG.plan.sharectx
    )]
    share_context: bool,
    /// The platform which the build will run on. The default is derived from the "eka.toml" via
    /// `platforms.build` and falls back to the platform eka itself was compiled on if unset.
    #[clap(long, default_value_t = CONFIG.platforms.build.to_string(), env = "EKA_PLATFORMS_BUILD")]
    build_platform: String,
    /// The platform the final artifact will run on. The default is derived from the "eka.toml" via
    /// `platforms.target` and falls back to the platform eka itself was compiled on if unset.
    #[clap(long, short, default_value_t = CONFIG.platforms.target.to_string(), env = "EKA_PLATFORMS_TARGET", name = "PLATFORM")]
    target: String,
    /// The platform used by legacy toolchains to specify the platform generated code will run on.
    #[clap(long, short, default_value_t = CONFIG.platforms.legacy_target.to_string(), hide = true, env = "EKA_PLATFORMS_LEGACY_TARGET")]
    legacy_target: String,
}

pub async fn run(storage: Option<&impl LocalStorage>, args: Args) -> anyhow::Result<()> {
    let mut tasks = JoinSet::new();
    let cache = git::cache_repo()?;
    let eval_tmp = tempfile::TempDir::with_prefix(".eval-atoms-")?;
    let mut atom_dirs: Vec<TempDir> = Vec::with_capacity(args.uri.len());
    let mut local_atom_dirs: Vec<PathBuf> = Vec::with_capacity(args.uri.len());
    let context = args.share_context;

    let platforms = config::Platforms {
        build: Cow::Owned(args.build_platform),
        target: Cow::Owned(args.target),
        legacy_target: Cow::Owned(args.legacy_target),
    };

    let local = eval_tmp.as_ref().join("local");
    let ekala = storage.map(|s| s.ekala_manifest());
    for uri in args.uri {
        if let Some(url) = uri.url().map(ToOwned::to_owned) {
            let dir = eval_tmp.as_ref().to_owned();
            let mut transport = url.get_transport()?;
            tasks.spawn_blocking(move || {
                let star = VersionReq::STAR;
                let req = uri.version().unwrap_or(&star);
                match url.get_highest_match(uri.label(), req, Some(&mut transport)) {
                    Some((version, _)) => {
                        let repo = &cache.to_thread_local();
                        match repo.cache_and_materialize_atom(
                            &url,
                            uri.label(),
                            &version,
                            &mut transport,
                            dir,
                        ) {
                            Ok(dir) => Some(dir),
                            Err(e) => {
                                tracing::error!("{}", e);
                                None
                            },
                        }
                    },
                    None => {
                        tracing::warn!(
                            label = %uri.label(),
                            url = %url,
                            requested = %req,
                            "skipped: couldn't acquire requested atom uri from remote"
                        );
                        None
                    },
                }
            });
        } else if let Some(storage) = storage
            && let Some(ekala) = &ekala
        {
            let ekala = match ekala {
                Ok(e) => e,
                Err(e) => return Err(anyhow!(e.to_string())),
            };
            let root = storage.ekala_root_dir()?;

            if !local.is_symlink() {
                #[cfg(unix)]
                std::os::unix::fs::symlink(root, &local)?;
            }

            if let Some(path) = ekala.set().packages().as_ref().get_by_left(uri.label()) {
                // FIXME: this entire block should probably be abstracted in library code
                // but the current implementation does not import into the cache, which we
                // will need to do for evaluatoin caching later.
                let full_path = local.join(path);
                let atom_nix = full_path.join(NIX_IMPORT_FILE);

                if !atom_nix.exists()
                    && let Ok(c) = std::fs::read(full_path.join(atom::LOCK_NAME.as_str()))
                    && let Ok::<Lockfile, _>(lock) = toml_edit::de::from_slice(c.as_slice())
                        .map_err(|e| tracing::error!("{}", e))
                    && let Some(url) = lock.locker().mirror()
                {
                    let repo = &cache.to_thread_local();
                    let mut transport = url.get_transport()?;
                    let mut remote = repo.ensure_remote(url, &mut transport)?;
                    let lock_atom = repo.resolve_atom_to_cache(
                        &mut remote,
                        lock.locker().label(),
                        lock.locker().version(),
                        &mut transport,
                    )?;
                    if let Some(entry) = repo
                        .find_commit(lock_atom.atom)?
                        .tree()?
                        .lookup_entry_by_path(NIX_IMPORT_FILE)?
                    {
                        std::fs::write(&atom_nix, entry.object()?.detach().data)?;
                    }
                }
                if atom_nix.exists() {
                    local_atom_dirs.push(full_path.to_path_buf());
                } else {
                    tracing::warn!("couldn't resolve lock expression; skipping local atom");
                }
            };
        } else {
            // TODO: implement setless execution
            tracing::warn!(
                label = %uri.label(),
                "There is no local set established, can't resolve local atom request"
            );
        }
    }
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tasks.shutdown().await;
        }
        _ = async {
            while let Some(dir) = tasks.join_next().await {
                match dir {
                    Ok(Some(dir)) => atom_dirs.push(dir),
                    Err(e) => tracing::error!("{}", e),
                    _ => {},
                }
            }
        } => {}
    }
    let eval_dirs = atom_dirs
        .iter()
        .map(|d| d.as_ref().to_path_buf())
        .chain(local_atom_dirs)
        .collect::<Vec<_>>();
    plan_atoms(eval_dirs, context, eval_tmp, platforms).await
}

async fn plan_atoms_with_shared_context(
    bin: PathBuf,
    workdir: impl AsRef<Path>,
    atom_dirs: Vec<PathBuf>,
    env: HashMap<String, String>,
    args: impl IntoIterator<Item = String>,
) -> io::Result<ExitStatus> {
    tokio::process::Command::new(&bin)
        .arg0(crate::NIXEC)
        .env_clear()
        .envs(&env)
        .current_dir(&workdir)
        .kill_on_drop(true)
        .args(
            atom_dirs
                .iter()
                .map(|p| {
                    p.strip_prefix(workdir.as_ref())
                        .unwrap_or(p.as_ref())
                        .join(git::NIX_IMPORT_FILE)
                        .to_string_lossy()
                        .into_owned()
                })
                .chain(args),
        )
        .spawn()?
        .wait()
        .await
}

async fn plan_atoms(
    atom_dirs: Vec<PathBuf>,
    shared_context: bool,
    workdir: TempDir,
    platforms: config::Platforms<'static>,
) -> anyhow::Result<()> {
    let bin = std::env::current_exe()?;
    if crate::is_same_exe(bin.as_ref())? {
        let value = serde_json::json!({
            "platforms": platforms
        });
        let config = json_digest::canonical_json(&value)?;

        let args = [
            "-A",
            git::NIX_ENTRY_KEY,
            "--argstr",
            "config",
            config.as_str(),
        ]
        .map(ToOwned::to_owned);

        let mut tasks = JoinSet::new();
        let filtered_env: HashMap<String, String> = std::env::vars()
            .filter(|(k, _)| k == "HOME" || k == "PATH" || k == "XDG_CACHE_HOME")
            .collect();
        if shared_context {
            tasks.spawn(async move {
                plan_atoms_with_shared_context(bin, workdir, atom_dirs, filtered_env, args).await
            });
        } else {
            for dir in &atom_dirs {
                let local = &workdir.as_ref().join("local");
                let local = dir.starts_with(local).then_some(&local);
                let mut child = tokio::process::Command::new(&bin)
                    .arg0(crate::NIXEC)
                    .env_clear()
                    .envs(&filtered_env)
                    .current_dir(local.unwrap_or(&dir))
                    .kill_on_drop(true)
                    .arg(if let Some(local) = local {
                        dir.strip_prefix(local)?.join(git::NIX_IMPORT_FILE)
                    } else {
                        git::NIX_IMPORT_FILE.into()
                    })
                    .args(args.iter().map(|p| p.as_str()))
                    .spawn()?;
                tasks.spawn(async move { child.wait().await });
            }
        }

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tasks.shutdown().await;
            }
            _ = async {
                while let Some(res) = tasks.join_next().await {
                    match res {
                        Ok(Ok(s)) => {
                            if s.success() {
                                tracing::info!("done: {}", s);
                            }
                        },
                        Ok(Err(ref e)) => tracing::error!("{}", e),
                        Err(ref e) => tracing::error!("{}", e),
                    }
                }
            } => {
                tracing::info!("all {} jobs completed", crate::NIXEC);
            }
        }
    } else {
        return Err(anyhow!(
            "binary was changed at runtime, not safe to re-exec!"
        ));
    };
    Ok(())
}
