#[cfg(feature = "git")]
use gix::ThreadSafeRepository;

use etcetera::BaseStrategy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use figment::{
    providers::{Env, Format, Toml},
    Figment, Metadata, Provider,
};

lazy_static::lazy_static! {
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
    pub static ref CONFIG: Config = load_config();
}

fn load_config() -> Config {
    Config::figment().extract().unwrap_or_default()
}

type Aliases<'a> = HashMap<&'a str, &'a str>;

#[derive(Deserialize, Serialize)]
pub struct Config {
    #[serde(borrow)]
    aliases: Aliases<'static>,
}

impl Config {
    pub fn aliases(&self) -> &Aliases {
        &self.aliases
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
        }
    }
}

impl Config {
    pub fn from<T: Provider>(provider: T) -> Result<Config, figment::Error> {
        Figment::from(provider).extract()
    }

    pub fn figment() -> Figment {
        let mut fig = Figment::from(Config::default());

        if let Ok(c) = etcetera::choose_base_strategy() {
            let config = c.config_dir().join("eka.toml");
            fig = fig.admerge(Toml::file(config));
        }

        #[cfg(feature = "git")]
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
