//! Tests for git storage operations.

use super::*;
use crate::test::harness::init_repo_and_remote;

//================================================================================================
// Tests
//================================================================================================

#[test]
fn init_repo() -> Result<(), anyhow::Error> {
    let (dir, _remote) = init_repo_and_remote()?;
    let repo = gix::open(dir.as_ref())?;
    let remote = repo.find_remote("origin")?;
    let mut transport = remote.get_transport().ok();
    remote.ekala_init(transport.as_mut())?;
    assert!(remote.ekala_genesis(transport.as_mut()).is_ok());
    Ok(())
}

#[test]
fn uninitialized_repo() -> Result<(), anyhow::Error> {
    let (dir, _remote) = init_repo_and_remote()?;
    let repo = gix::open(dir.as_ref())?;
    let remote = repo.find_remote("origin")?;
    assert!(remote.ekala_genesis(None).is_err());
    Ok(())
}
