use std::sync::atomic::AtomicBool;

use gix::progress::Discard;
use gix::revision::walk::Info;
use gix::{ObjectId, ThreadSafeRepository};
use proc_macro::TokenStream;
use quote::quote;

const LOCK_LABEL: &str = "nix-lock";
const LOCK_MAJOR: u64 = 0;
const LOCK_MINOR: u64 = 1;
const LOCK_PATCH: u64 = 0;

/// Computes Eka's repository root commit hash at compile time
#[proc_macro]
pub fn eka_origin_info(_input: TokenStream) -> TokenStream {
    let root_hash = match compute_eka_root_hash() {
        Ok(hash) => hash,
        Err(e) => panic!("Failed to compute Eka root hash: {}", e),
    };

    let origin_url = eka_origin().to_string();
    let url = origin_url.as_str();

    // Convert [u8; 20] to token streams for each byte
    let root_tokens = root_hash.iter().map(|&byte| quote! { #byte });
    let rev = lock_rev();
    let rev_tokens = rev.iter().map(|&byte| quote! { #byte });

    quote! {
        const LOCK_MAJOR: u64 = #LOCK_MAJOR;
        const LOCK_MINOR: u64 = #LOCK_MINOR;
        const LOCK_PATCH: u64 = #LOCK_PATCH;
        pub(crate) const LOCK_LABEL: &str = #LOCK_LABEL;
        pub(crate) const LOCK_REV: [u8; 20] = [#(#rev_tokens),*];
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

fn lock_rev() -> [u8; 20] {
    let version = format!("{}.{}.{}", LOCK_MAJOR, LOCK_MINOR, LOCK_PATCH);
    let revspec = format!("refs/eka/atoms/{}/{}", LOCK_LABEL, &version);

    let repo = get_repo().to_thread_local();
    let remote = default_remote();
    let mut remote = repo
        .try_find_remote_without_url_rewrite(remote.as_str())
        .and_then(|x| x.ok())
        .unwrap_or_else(|| panic!("couldn't open default remote: {}", remote));
    remote
        .replace_refspecs([revspec.as_str()], gix::remote::Direction::Fetch)
        .ok();

    if let Some(ObjectId::Sha1(bytes)) = remote
        .connect(gix::remote::Direction::Fetch)
        .ok()
        .and_then(|c| c.prepare_fetch(Discard, Default::default()).ok())
        .and_then(|q| {
            q.with_write_packed_refs_only(true)
                .receive(Discard, &AtomicBool::new(false))
                .ok()
        })
        .and_then(|o| {
            o.ref_map.remote_refs.iter().find_map(|r| {
                let (n, p, _) = r.unpack();

                p.and_then(|t| if n == revspec { Some(t.into()) } else { None })
            })
        })
    {
        bytes
    } else {
        panic!(
            "aborting compilation. eka lock rev could not be calculated, make sure you have \
             published first: ::{}@{}",
            LOCK_LABEL, &version
        )
    }
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
