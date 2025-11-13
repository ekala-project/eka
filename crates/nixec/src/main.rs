//! `nixec` is a sandboxed version of `nix-instantiate`.
//!
//! This utility wraps `nix-instantiate` in a `birdcage` sandbox to restrict its
//! access to the file system and environment variables, enhancing security during
//! Nix expression evaluation.

use std::path::{Path, PathBuf};
use std::process::{Command as UnsafeCommand, ExitCode};
use std::{env, fs};

use birdcage::process::Command;
use birdcage::{Birdcage, Exception, Sandbox};
use thiserror::Error;

//================================================================================================
// Constants
//================================================================================================

const SSL_CERT_PATH: &str = "/nix/var/nix/profiles/default/etc/ssl/certs/ca-bundle.crt";
const RESOLV_CONF_PATH: &str = "/etc/resolv.conf";
const DEV_NULL_PATH: &str = "/dev/null";

//================================================================================================
// Types
//================================================================================================

/// Configuration for nixec execution.
#[derive(Debug)]
struct NixecConfig {
    nix_dir: PathBuf,
    git_dir: PathBuf,
    utils_dir: PathBuf,
    nix_store: PathBuf,
    nix_config: String,
}

/// Represents the possible errors that can occur during `nixec` execution.
#[derive(Error, Debug)]
pub enum NixecError {
    /// Error indicating that the `nix` executable could not be found in the system's PATH.
    #[error("No `{0}` executable in PATH")]
    NoBin(String),
    /// Error originating from the `birdcage` sandboxing library.
    #[error(transparent)]
    ExceptionFailed(#[from] birdcage::error::Error),
    /// Error from I/O operations, typically when executing a command.
    #[error(transparent)]
    CommandFailed(#[from] std::io::Error),
    /// Error for UTF-8 conversion failures.
    #[error(transparent)]
    Utf8Error(#[from] std::string::FromUtf8Error),
    /// Error for when the Nix store path could not be determined.
    #[error("Failed to determine nix store path")]
    StorePath,
}

//================================================================================================
// Functions
//================================================================================================

/// Sets up the necessary paths and configuration for nixec.
fn setup_paths() -> Result<NixecConfig, NixecError> {
    let nix_dir = bin_dir("nix")?;
    let git_dir = bin_dir("git")?;
    let utils_dir = bin_dir("coreutils")?;
    let nix_instantiate = nix_dir.join("nix-instantiate");
    let nix = nix_dir.join("nix");

    let nix_store: PathBuf = String::from_utf8(
        UnsafeCommand::new(&nix_instantiate)
            .args(["--eval", "--raw", "--expr", "builtins.storeDir"])
            .output()?
            .stdout,
    )
    .map_err(|_| NixecError::StorePath)?
    .trim()
    .into();

    let nix_config: String = String::from_utf8(
        UnsafeCommand::new(&nix)
            .args(["config", "show"])
            .output()?
            .stdout,
    )?
    .trim()
    .lines()
    .chain([
        format!("ssl-cert-file = {}", SSL_CERT_PATH).as_str(),
        "pure-eval = false",
        "restrict-eval = true",
        // FIXME: hardcoded for now, should allow setting externally
        "allowed-uris = git+https: https: ssh: git+ssh:",
    ])
    .collect::<Vec<&str>>()
    .join("\n");

    Ok(NixecConfig {
        nix_dir,
        git_dir,
        utils_dir,
        nix_store,
        nix_config,
    })
}

/// Configures the sandbox with necessary exceptions and environment variables.
fn configure_sandbox(config: &NixecConfig) -> Result<Birdcage, NixecError> {
    let cwd = env::current_dir()?;
    let mut sandbox = Birdcage::new();

    if let Ok(home) = std::env::var("HOME") {
        let cache = std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or(PathBuf::from(home).join(".cache"));
        unsafe { env::set_var("XDG_CACHE_HOME", &cache) };
        sandbox
            .add_exception(Exception::Environment("XDG_CACHE_HOME".into()))?
            .add_exception(Exception::WriteAndRead(cache.join("nix")))?;
    };

    let nix_root = config
        .nix_store
        .parent()
        .map(Path::to_path_buf)
        .ok_or(NixecError::StorePath)?;

    unsafe { env::set_var("NIX_CONFIG", &config.nix_config) };
    unsafe {
        env::set_var(
            "PATH",
            format!(
                "{}:{}:{}",
                config.nix_dir.display(),
                config.git_dir.display(),
                config.utils_dir.display()
            ),
        )
    };
    unsafe { env::set_var("GIT_SSL_CAINFO", SSL_CERT_PATH) };
    unsafe { env::set_var("NIX_PATH", format!("eval={}", cwd.display())) };
    sandbox
        .add_exception(Exception::Environment("HOME".into()))?
        .add_exception(Exception::Environment("NIX_CONFIG".into()))?
        .add_exception(Exception::Environment("NIX_PATH".into()))?
        .add_exception(Exception::Environment("PATH".into()))?
        .add_exception(Exception::Environment("GIT_SSL_CAINFO".into()))?
        .add_exception(Exception::Read(cwd))?
        .add_exception(Exception::Read(RESOLV_CONF_PATH.into()))?
        .add_exception(Exception::ExecuteAndRead(nix_root))?
        .add_exception(Exception::ExecuteAndRead(config.nix_dir.clone()))?
        .add_exception(Exception::ExecuteAndRead(config.git_dir.clone()))?
        .add_exception(Exception::ExecuteAndRead(config.utils_dir.clone()))?
        .add_exception(Exception::WriteAndRead(DEV_NULL_PATH.into()))?
        .add_exception(Exception::Networking)?;

    Ok(sandbox)
}

/// Runs the nix-instantiate command within the configured sandbox.
fn run_command_in_sandbox(
    config: &NixecConfig,
    sandbox: Birdcage,
    args: &[String],
) -> Result<ExitCode, NixecError> {
    let nix_instantiate = config.nix_dir.join("nix-instantiate");
    let mut command = Command::new(nix_instantiate);
    command.args(args);

    let output = sandbox.spawn(command)?.wait_with_output()?;
    println!("{}", String::from_utf8(output.stdout)?);
    println!("{}", String::from_utf8(output.stderr)?);

    Ok(ExitCode::from(output.status.code().unwrap_or(1) as u8))
}

/// Runs nixec with the given arguments. This is the main logic that can be called for testing.
pub fn run_nixec(args: Vec<String>) -> Result<ExitCode, NixecError> {
    let config = setup_paths()?;
    let sandbox = configure_sandbox(&config)?;
    let sandbox_args = &args[1..]; // Assuming args[0] is program name
    run_command_in_sandbox(&config, sandbox, sandbox_args)
}

/// The main entry point for the `nixec` executable.
fn main() -> anyhow::Result<ExitCode> {
    let args: Vec<String> = env::args().collect();
    run_nixec(args).map_err(Into::into)
}

/// Locates the directory containing a given executable.
///
/// This function searches the directories listed in the `PATH` environment variable
/// to find the specified executable and returns its parent directory.
fn bin_dir(exec_name: &str) -> Result<PathBuf, NixecError> {
    env::var_os("PATH")
        .and_then(|paths| {
            env::split_paths(&paths)
                .filter_map(|dir| {
                    let full_path = dir.join(exec_name);
                    if full_path.is_file() {
                        fs::canonicalize(full_path)
                            .ok()
                            .and_then(|p| p.parent().map(Path::to_path_buf))
                    } else {
                        None
                    }
                })
                .next()
        })
        .ok_or(NixecError::NoBin(exec_name.into()))
}
