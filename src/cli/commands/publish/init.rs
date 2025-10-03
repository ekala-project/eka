use crate::cli::store::Detected;

pub(in super::super) fn run(store: Detected, args: super::StoreArgs) -> anyhow::Result<()> {
    #[allow(clippy::single_match)]
    match store {
        Detected::Git(repo) => {
            use atom::store::Init;
            let repo = repo.to_thread_local();
            let remote = repo.find_remote(args.git.remote.as_str())?;
            remote.ekala_init(None)?
        },
        _ => {},
    }
    Ok(())
}
