use toml_edit::de::from_str;

use super::*;

const TOML_LOCK: &str = include_str!("test/atom.lock");

#[test]
fn parse_lock() -> anyhow::Result<()> {
    let _lock: Lockfile = toml_edit::de::from_str(&TOML_LOCK)?;

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
    assert!(invalid.is_err());

    Ok(())
}
