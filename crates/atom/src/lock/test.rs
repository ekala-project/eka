use toml_edit::de::from_str;

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

    let _deps: Manifest = from_str(manifest)?;

    Ok(())
}
