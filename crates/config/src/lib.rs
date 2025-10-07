use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

use etcetera::BaseStrategy;
use figment::providers::{Env, Format, Toml};
use figment::{Figment, Metadata, Provider};
use gix::ThreadSafeRepository;
use serde::{Deserialize, Serialize};

/// Provide a lazyily instantiated static reference to
/// a config object parsed from canonical locations
/// so that applications have immutable access to it from
/// anywhere without ever having to parse the config more
/// than once.
///
/// For efficiency, all collections in the Config contain
/// references to values owned by the deserializer instead
/// of owned data, ensuring cheap copying where ownership
/// is required.
pub static CONFIG: LazyLock<Config> = LazyLock::new(load_config);

fn load_config() -> Config {
    Config::figment().extract().unwrap_or_default()
}

type Aliases<'a> = HashMap<&'a str, &'a str>;

#[derive(Deserialize, Serialize)]
pub struct Config {
    #[serde(borrow)]
    aliases: Aliases<'static>,
    pub cache: CacheConfig,
}

#[derive(Deserialize, Serialize)]
pub struct CacheConfig {
    pub root_dir: PathBuf,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            root_dir: get_cache(),
        }
    }
}

impl Config {
    pub fn aliases(&self) -> &Aliases<'_> {
        &self.aliases
    }
}

fn get_cache() -> PathBuf {
    if let Ok(c) = etcetera::choose_base_strategy() {
        c.cache_dir().join("eka")
    } else {
        std::env::temp_dir().join("eka")
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            aliases: HashMap::from_iter([
                ("gh", "github.com"),
                ("gl", "gitlab.com"),
                ("cb", "codeberg.org"),
                ("bb", "bitbucket.org"),
                ("sh", "sr.ht"),
                ("pkgs", "gh:nixos/nixpkgs"),
            ]),
            cache: CacheConfig::default(),
        }
    }
}

impl Config {
    pub fn from<T: Provider>(provider: T) -> Result<Config, Box<figment::Error>> {
        Figment::from(provider).extract().map_err(Box::new)
    }

    pub fn figment() -> Figment {
        let mut fig = Figment::from(Config::default());

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
}

impl Provider for Config {
    fn metadata(&self) -> figment::Metadata {
        Metadata::named("Eka CLI Config")
    }

    fn data(
        &self,
    ) -> Result<figment::value::Map<figment::Profile, figment::value::Dict>, figment::Error> {
        figment::providers::Serialized::defaults(Config::default()).data()
    }
}
