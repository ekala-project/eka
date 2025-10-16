use tempfile::TempDir;

use super::*;

//================================================================================================
// Functions
//================================================================================================

pub(crate) fn init_repo_and_remote() -> Result<(TempDir, TempDir), anyhow::Error> {
    use gix::actor::SignatureRef;
    use gix::config::{File, Source};
    let sig = SignatureRef::default();
    let repo_dir = tempfile::tempdir()?;
    let remote_dir = tempfile::tempdir()?;
    let repo = gix::init(repo_dir.as_ref())?;
    let remote = gix::init_bare(remote_dir.as_ref())?;
    let no_parents: Vec<gix::ObjectId> = vec![];
    let init = remote.commit_as(
        sig,
        sig,
        "HEAD",
        "init",
        repo.empty_tree().id(),
        no_parents.clone(),
    )?;
    remote.commit_as(sig, sig, "HEAD", "2nd", repo.empty_tree().id(), vec![
        init.detach(),
    ])?;

    let config_file = repo.git_dir().join("config");
    let mut config = File::from_path_no_includes(config_file.clone(), Source::Local)?;
    let mut repo_remote =
        repo.remote_at(format!("file://{}", remote.git_dir().display()).as_str())?;
    repo_remote.save_as_to("origin", &mut config)?;
    config.set_raw_value(&"user.email", "eka")?;
    config.set_raw_value(&"user.name", "eka")?;
    let mut file = std::fs::File::create(config_file)?;
    config.write_to(&mut file)?;
    Ok((repo_dir, remote_dir))
}

#[test]
fn init_repo() -> Result<(), anyhow::Error> {
    let (dir, _remote) = init_repo_and_remote()?;
    let repo = gix::open(dir.as_ref())?;
    let remote = repo.find_remote("origin")?;
    let mut transport = remote.get_transport().ok();
    remote.ekala_init(transport.as_mut())?;
    assert!(remote.ekala_root(transport.as_mut()).is_ok());
    Ok(())
}

#[test]
fn uninitialized_repo() -> Result<(), anyhow::Error> {
    let (dir, _remote) = init_repo_and_remote()?;
    let repo = gix::open(dir.as_ref())?;
    let remote = repo.find_remote("origin")?;
    assert!(remote.ekala_root(None).is_err());
    Ok(())
}
