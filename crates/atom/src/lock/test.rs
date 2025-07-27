use toml_edit::de::from_str;

use super::Lock;
use crate::Manifest;

#[test]
fn parse_deps() -> anyhow::Result<()> {
    let manifest = r#"
        [atom]
        id = "test"
        version = "0.1.0"

        [deps.bonds]
        term = "./term"
        wm =  "./wm"
        nix = "./nix"

        [deps.bonds.mine]
        version = "^1"
        url = "https://my.com/repo.git"

        [deps.pins.hm-config]
        from = "nix"
        get = "hm-module"
        entry = "modules"

        [deps.srcs.example]
        url = "https://example.com"
    "#;

    let lock = r#"
        version = "1"

        [[eval]]
        type = "atom"
        id = "term"
        version = "0.1.1"
        rev = "e4f0949e04edfeb30a8e4c4df80def1e5012828f"

        [[eval]]
        id = "wm"
        version = "0.1.1"
        rev = "82894264b0b1337d49c35a05af98eacec861074d"
        type = "atom"

        [[eval]]
        id = "nix"
        version = "0.1.2"
        rev = "bca8295431846ed43bdbe9d95a8b8958785245e6"
        type = "atom"

        [[eval]]
        name = "hm-config"
        from = "nix"
        get = "hm-module"
        entry = "modules"
        type = "pin+from"

        [[eval]]
        id = "network"
        version = "0.1.0"
        path = "os/network"
        rev = "bca8295431846ed43bdbe9d95a8b8958785245e6"
        type = "atom"

        [[eval]]
        id = "virt"
        version = "0.1.0"
        path = "os/virt"
        rev = "bca8295431846ed43bdbe9d95a8b8958785245e6"
        type = "atom"

        [[eval]]
        name = "eval-config"
        entry = "nixos/lib/eval-config.nix"
        from = "nix"
        get = "nixpkgs"
        type = "pin+from"

        [[eval]]
        name = "microvm-module"
        url = "https://github.com/microvm-nix/microvm.nix/archive/80bddbd51fda2c71d83449f5927e4d72a2eb0d89.tar.gz"
        checksum = "sha256:0lkjn8q6p0c18acj43pj1cbiyixnf98wvkbgppr5vz73qkypii2g"
        entry = "nixos-modules/host"
        type = "pin+tar"

        [[build]]
        name = "registry"
        url = "https://raw.githubusercontent.com/NixOS/flake-registry/refs/heads/master/flake-registry.json"
        checksum = "sha256-hClMprWwiEQe7mUUToXZAR5wbhoVFi+UuqLL2K/eIPw="
    "#;

    let _deps: Manifest = from_str(manifest)?;
    let _lock: Lock = from_str(lock)?;

    Ok(())
}
