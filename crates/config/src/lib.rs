//! Manages application configuration by loading settings from standard locations.
//!
//! This crate provides a unified configuration object (`Config`) that aggregates
//! settings from files and environment variables, making them accessible
//! globally via a lazily initialized static reference (`CONFIG`).

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

use etcetera::BaseStrategy;
use figment::providers::{Env, Format, Toml};
use figment::{Figment, Metadata, Provider};
use gix::ThreadSafeRepository;
use serde::{Deserialize, Serialize};

/// The default configuration values
const DEFAULT_TOML_CONFIG: &str = include_str!("./eka.default.toml");

//================================================================================================
// Statics
//================================================================================================

/// Provides a lazily instantiated static reference to the application `Config`.
///
/// This static variable ensures that configuration is parsed only once from
/// canonical locations and then made immutably available throughout the
/// application's lifecycle.
pub static CONFIG: LazyLock<Config> = LazyLock::new(load_config);

//================================================================================================
// Types
//================================================================================================

#[derive(Deserialize, Serialize, Default)]
pub struct AtomConfig<'a> {
    #[serde(borrow)]
    default: AtomDefaults<'a>,
}

#[derive(Deserialize, Serialize, Default)]
pub struct AtomDefaults<'a> {
    #[serde(borrow)]
    composer: Cow<'a, str>,
}

/// Defines cache-related configuration settings.
#[derive(Deserialize, Serialize)]
pub struct CacheConfig {
    /// The root directory for storing cached data.
    pub root: PathBuf,
}

/// Represents the application's primary configuration structure.
#[derive(Deserialize, Serialize, Default)]
pub struct Config {
    /// A map of command aliases.
    #[serde(borrow)]
    uri: Uri<'static>,
    /// Cache-related settings.
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(borrow)]
    pub atom: AtomConfig<'static>,
}

#[derive(Deserialize, Serialize, Default)]
pub struct Uri<'a> {
    /// A map of uri aliases.
    #[serde(borrow)]
    aliases: Aliases<'a>,
}

/// A type alias for a hash map of borrowed string slices, used for command aliases.
type Aliases<'a> = HashMap<Cow<'a, str>, Cow<'a, str>>;

//================================================================================================
// Impls
//================================================================================================

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            root: get_cache_dir(),
        }
    }
}

impl Config {
    /// Returns a reference to the command aliases.
    pub fn aliases(&self) -> &Aliases<'_> {
        &self.uri.aliases
    }

    /// Constructs a `Figment` instance for configuration loading.
    ///
    /// This method builds a configuration provider by layering default settings,
    /// user-specific configuration files, and environment variables.
    pub fn figment() -> Figment {
        let mut fig = Figment::from(Config::default()).merge(Toml::string(DEFAULT_TOML_CONFIG));

        if let Ok(c) = etcetera::choose_base_strategy() {
            let config = c.config_dir().join("eka.toml");
            fig = fig.admerge(Toml::file(config));
        }

        if let Ok(r) = ThreadSafeRepository::discover(".") {
            let repo_config = r.git_dir().join("info/eka.toml");
            fig = fig.admerge(Toml::file(repo_config));
        };

        fig.admerge(Env::prefixed("EKA_"))
    }

    /// Creates a `Config` instance from a given provider.
    pub fn from<T: Provider>(provider: T) -> Result<Config, Box<figment::Error>> {
        Figment::from(provider).extract().map_err(Box::new)
    }
}

impl Provider for Config {
    fn metadata(&self) -> figment::Metadata {
        Metadata::named("Eka CLI Config")
    }

    fn data(
        &self,
    ) -> Result<figment::value::Map<figment::Profile, figment::value::Dict>, figment::Error> {
        figment::providers::Serialized::defaults(self).data()
    }
}

//================================================================================================
// Functions
//================================================================================================

/// Determines the appropriate cache directory based on the operating system.
fn get_cache_dir() -> PathBuf {
    if let Ok(c) = etcetera::choose_base_strategy() {
        c.cache_dir().join("eka")
    } else {
        std::env::temp_dir().join("eka")
    }
}

/// Loads the application configuration using the default `Figment` provider.
///
/// This function is used to initialize the `CONFIG` static variable.
fn load_config() -> Config {
    Config::figment().extract().unwrap_or_else(|e| {
        tracing::error!(error = %e, "problem loading config from default sources, falling back to nearly empty configuration");
        Config::default()
    })
}
