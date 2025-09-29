use toml_edit::de::from_str;

use super::*;

#[test]
fn parse_lock() -> anyhow::Result<()> {
    let lock_str = r#"
version = 1

[[deps]]
type = "atom"
tag = "nix"
id = "36e2c6c52d4b3b10983d79367aeef03b0f47578ade287b08e89f68b647af26a3" # currently not validated
version = "0.1.2"
rev = "bca8295431846ed43bdbe9d95a8b8958785245e6"

[[deps]]
type = "atom"
tag = "home"
id = "4ef4c265e176136f4c6cc348c61489b92f163ee571d08ea12c2268e9d2c4c790"
version = "0.1.8"
rev = "795ae541b7fd67dd3c6e1a9dddf903312696aa17"
url = "https://git.example.com/my-repo.git"

[[deps]]
type = "atom"
id = "734538b25b6840e47fbd491550de267b985255b5b842b41baf4ccf5afd224c3d"
tag = "users"
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
name = "hm-module"
url = "https://github.com/nix-community/home-manager.git"
rev = "d0300c8808e41da81d6edfc202f3d3833c157daf"
path = "nixos"
type = "pin+git"

[[deps]]
name = "foks"
url = "https://raw.githubusercontent.com/NixOS/nixpkgs/393d5e815a19acad8a28fc4b27085e42c483b4f6/pkgs/by-name/fo/foks/package.nix"
type = "pin"
hash = "sha256:1spc2lsx16xy612lg8rsyd34j9fy6kmspxcvcfmawkxmyvi32g9v"

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
