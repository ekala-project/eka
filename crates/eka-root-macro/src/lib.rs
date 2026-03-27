use gix::revision::walk::Info;
use gix::{ObjectId, ThreadSafeRepository};
use proc_macro::TokenStream;
use quote::quote;

const LOCK_LABEL: &str = "nix-lock";

/// Computes Eka's repository root commit hash at compile time
#[proc_macro]
pub fn eka_origin_info(_input: TokenStream) -> TokenStream {
    let root_hash = if let Ok(var) = std::env::var("EKA_ROOT_COMMIT_HASH") {
        hex::decode(var)
            .ok()
            .and_then(|v| v.try_into().ok())
            .expect("set `EKA_ROOT_COMMIT_HASH` is not a valid sha")
    } else {
        match compute_eka_root_hash() {
            Ok(hash) => hash,
            Err(e) => panic!("Failed to compute Eka root hash: {}", e),
        }
    };

    let root_tokens = root_hash.iter().map(|&byte| quote! { #byte });
    let origin_url = if let Ok(var) = std::env::var("EKA_ORIGIN_URL") {
        gix::url::parse(var.as_bytes().into()).expect("EKA_ORIGIN_URL is not a valid url");
        var
    } else {
        eka_origin().to_string()
    };
    let url = origin_url.as_str();

    quote! {
        pub(crate) const LOCK_LABEL: &str = #LOCK_LABEL;
        pub(crate) const EKA_ORIGIN_URL: &str = #url;
        pub(crate) const EKA_ROOT_COMMIT_HASH: [u8; 20] = [#(#root_tokens),*];
    }
    .into()
}

fn compute_eka_root_hash() -> Result<[u8; 20], Box<dyn std::error::Error>> {
    let repo = get_repo().to_thread_local();
    let head = repo.head_commit()?;
    let root = calculate_origin(&head)?;

    Ok(root)
}

fn eka_origin() -> gix::Url {
    let remote = default_remote();
    get_repo()
        .to_thread_local()
        .try_find_remote_without_url_rewrite(remote.as_str())
        .and_then(|r| r.ok())
        .and_then(|r| r.url(gix::remote::Direction::Push).map(ToOwned::to_owned))
        .expect("aborting compilation. cannot detect origin url of eka repository")
}

fn default_remote() -> String {
    use gix::remote::Direction;
    get_repo()
        .to_thread_local()
        .remote_default_name(Direction::Push)
        .map(|s| s.to_string())
        .unwrap_or("origin".into())
}

fn get_repo() -> ThreadSafeRepository {
    use gix::discover::upwards::Options;
    use gix::sec::Trust;
    use gix::sec::trust::Mapping;
    let opts = Options {
        required_trust: Trust::Full,
        ..Default::default()
    };
    ThreadSafeRepository::discover_opts(".", opts, Mapping::default())
        .expect("repo could not be opened, are you in a detached head?")
}

fn calculate_origin(commit: &gix::Commit) -> Result<[u8; 20], gix::revision::walk::Error> {
    use gix::revision::walk::Sorting;
    use gix::traverse::commit::simple::CommitTimeOrder;
    let mut walk = commit
        .ancestors()
        .use_commit_graph(true)
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::OldestFirst))
        .all()?;

    while let Some(Ok(
        info @ Info {
            id: ObjectId::Sha1(bytes),
            ..
        },
    )) = walk.next()
    {
        if info.parent_ids.is_empty() {
            return Ok(bytes);
        }
    }

    panic!(
        "aborting compilation. eka root hash cannot be computed. make sure you are not in a \
         detached head state"
    )
}
