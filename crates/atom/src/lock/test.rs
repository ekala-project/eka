//! Tests for lockfile parsing and validation.

use toml_edit::de::from_str;

use super::*;

/// A sample lockfile for testing purposes.
const TOML_LOCK: &str = include_str!("test/atom.lock");

/// Tests that a valid lockfile can be parsed and an invalid one is rejected.
#[test]
fn parse_lock() -> anyhow::Result<()> {
    // Test that a valid lockfile can be parsed successfully.
    let _lock: Lockfile = toml_edit::de::from_str(TOML_LOCK)?;

    // Test that a lockfile with an invalid dependency is rejected.
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
    assert!(
        invalid.is_err(),
        "Parsing should fail for a lockfile with invalid fields"
    );

    Ok(())
}
