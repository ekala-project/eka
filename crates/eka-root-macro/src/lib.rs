use gix::ThreadSafeRepository;
use proc_macro::TokenStream;
use quote::quote;

/// Computes Eka's repository root commit hash at compile time
#[proc_macro]
pub fn eka_root_hash(_input: TokenStream) -> TokenStream {
    let root_hash = match compute_eka_root_hash() {
        Ok(hash) => hash,
        Err(e) => panic!("Failed to compute Eka root hash: {}", e),
    };

    quote! {
        const EKA_ROOT_COMMIT_HASH: &str = #root_hash;
    }
    .into()
}

fn compute_eka_root_hash() -> Result<String, Box<dyn std::error::Error>> {
    let repo = get_repo()?.to_thread_local();
    let head = repo.head_commit()?;
    let root = calculate_origin(&head)?;

    Ok(root.to_string())
}

fn get_repo() -> Result<ThreadSafeRepository, Box<gix::discover::Error>> {
    use gix::discover::upwards::Options;
    use gix::sec::Trust;
    use gix::sec::trust::Mapping;
    let opts = Options {
        required_trust: Trust::Full,
        ..Default::default()
    };
    ThreadSafeRepository::discover_opts(".", opts, Mapping::default()).map_err(Box::new)
}

fn calculate_origin(commit: &gix::Commit) -> Result<gix::ObjectId, gix::revision::walk::Error> {
    use gix::revision::walk::Sorting;
    use gix::traverse::commit::simple::CommitTimeOrder;
    let mut walk = commit
        .ancestors()
        .use_commit_graph(true)
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::OldestFirst))
        .all()?;

    while let Some(Ok(info)) = walk.next() {
        if info.parent_ids.is_empty() {
            return Ok(info.id);
        }
    }

    panic!(
        "aborting compilation. eka root hash cannot be computed. make sure you are not in a \
         detached head state"
    )
}
