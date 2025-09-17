use toml_edit::de::from_str;

use super::*;

#[test]
fn parse_lock() -> anyhow::Result<()> {
    let lock_str = r#"
version = 1

[[deps]]
type = "atom"
id = "nix"
version = "0.1.2"
rev = "bca8295431846ed43bdbe9d95a8b8958785245e6"

[[deps]]
type = "atom"
id = "users"
version = "0.1.4"
rev = "a460f02d145b870bb4f8762fd7a163afee68512e"
path = "hm/usr"

[[deps]]
type = "from"
name = "eval-config"
path = "nixos/lib/eval-config.nix"
from = "nix"
get = "nixpkgs"

[[deps]]
type = "pin+tar"
name = "microvm-module"
url = "https://github.com/microvm-nix/microvm.nix/archive/80bddbd51fda2c71d83449f5927e4d72a2eb0d89.tar.gz"
hash = "sha256:0lkjn8q6p0c18acj43pj1cbiyixnf98wvkbgppr5vz73qkypii2g"
path = "nixos-modules/host"

[[srcs]]
name = "registry"
url = "https://raw.githubusercontent.com/NixOS/flake-registry/refs/heads/master/flake-registry.json"
type = "build"
hash = "sha256-hClMprWwiEQe7mUUToXZAR5wbhoVFi+UuqLL2K/eIPw="
"#;

    let lock: Result<Lockfile, _> = from_str(lock_str);

    // Verify conditional validation
    let invalid_lock_str = r#"
version = 1

[[deps]]
type = "pin"
name = "invalid"
url = "file://local"
checksum = "stub"
from = "nix"
"#;

    let invalid: Result<Lockfile, _> = from_str(invalid_lock_str);
    assert!(lock.is_ok());
    assert!(invalid.is_err());

    Ok(())
}
