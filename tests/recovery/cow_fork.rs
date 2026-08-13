use tempfile::tempdir;
use tokenzero_core::sha256_hex;
use tokenzero_recovery::{
    RecoveryStore,
    context_view::{AsOf, ContextProjection},
    cow_fork::{BranchLedgerAction, BranchLedgerEntry, CowForkError, CowSession},
    prefix_stability::{CacheModelTier, CacheablePrefix, PrefixStabilityGuard},
};

fn projection(rendered: &str, cache_breakpoint: bool) -> ContextProjection {
    ContextProjection {
        rendered: rendered.into(),
        stable_prefix: rendered.into(),
        stable_prefix_sha256: sha256_hex(rendered),
        stable_prefix_tokens: 1,
        input_tokens: 1,
        working_set_tokens: 0,
        working_set_ids: Vec::new(),
        hot_tail_ids: Vec::new(),
        evicted_ids: Vec::new(),
        as_of: Some(AsOf::Turn(10)),
        cache_breakpoint,
    }
}

#[test]
fn forks_only_at_breakpoint_and_shares_prefix_allocation() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery.json")));
    let ordinary = projection(
        "SYSTEM tools=v1
",
        false,
    );
    assert!(matches!(
        CowSession::from_breakpoint("root", &ordinary),
        Err(CowForkError::NotAtCacheBreakpoint)
    ));

    let breakpoint = projection(
        "SYSTEM tools=v1
",
        true,
    );
    let parent = CowSession::from_breakpoint("root", &breakpoint).unwrap();
    let child = parent.fork(&mut store, "branch-a").unwrap();
    assert!(parent.shares_prefix_with(&child));
    assert_eq!(parent.breakpoint_sha256(), child.breakpoint_sha256());
    assert_eq!(
        parent.breakpoint_sha256(),
        "237db125e89063a84a2e0a2bd5d966d97291080f9df2c2e35c573fa41d16de83"
    );
    assert_eq!(child.cost().novelty_bytes, 0);
    assert_eq!(
        child.cost().full_replay_bytes,
        child.cost().shared_prefix_bytes
    );

    let mut guard = PrefixStabilityGuard::default();
    for rendered in [parent.rendered(), child.rendered()] {
        guard
            .observe_prefix(
                &CacheablePrefix {
                    bytes: rendered,
                    cache_breakpoint: true,
                    blocks_per_turn: Default::default(),
                },
                CacheModelTier::OlderSonnet,
            )
            .expect("forked cached prefix remains byte-stable");
    }
}

#[test]
fn branch_ledger_is_ref_backed_and_discard_restore_roundtrips() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery.json")));
    let parent = CowSession::from_breakpoint(
        "root",
        &projection(
            "SYSTEM tools=v1
",
            true,
        ),
    )
    .unwrap();
    let mut child = parent.fork(&mut store, "branch-a").unwrap();
    let first_ref = child
        .append(
            &mut store,
            "user: alpha
",
        )
        .unwrap();
    let second_ref = child
        .append(
            &mut store,
            "assistant: beta
",
        )
        .unwrap();
    let actions = child
        .ledger_refs()
        .iter()
        .map(|ledger_ref| {
            let expanded = store.expand(ledger_ref, Some("raw"), None, None, None, None);
            assert!(expanded.found);
            serde_json::from_str::<BranchLedgerEntry>(&expanded.content)
                .unwrap()
                .action
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actions,
        vec![
            BranchLedgerAction::Fork,
            BranchLedgerAction::Append,
            BranchLedgerAction::Append,
        ]
    );
    let before = child.rendered();
    let cost = child.cost();
    assert_eq!(
        cost.novelty_bytes,
        "user: alpha
assistant: beta
"
        .len()
    );
    assert!(cost.novelty_bytes < cost.full_replay_bytes);

    let restore = child.discard(&mut store).unwrap();
    assert_eq!(
        child.rendered(),
        "SYSTEM tools=v1
"
    );
    let ledger = store.expand(
        &restore.discard_ledger_ref,
        Some("raw"),
        None,
        None,
        None,
        None,
    );
    assert!(ledger.found);
    let entry: BranchLedgerEntry = serde_json::from_str(&ledger.content).unwrap();
    assert_eq!(entry.action, BranchLedgerAction::Discard);
    assert_eq!(entry.record_refs, vec![first_ref, second_ref]);

    child.restore(&mut store, &restore).unwrap();
    assert_eq!(child.rendered().as_bytes(), before.as_bytes());
    assert_eq!(child.cost(), cost);
}

#[test]
fn checkpoint_makes_novelty_shareable_and_preserves_prefix_golden() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery.json")));
    let parent = CowSession::from_breakpoint(
        "root",
        &projection(
            "SYSTEM tools=v1
",
            true,
        ),
    )
    .unwrap();
    let mut branch = parent.fork(&mut store, "branch-a").unwrap();
    branch
        .append(
            &mut store,
            "user: novelty
",
        )
        .unwrap();
    assert!(matches!(
        branch.fork(&mut store, "illegal"),
        Err(CowForkError::NotAtCacheBreakpoint)
    ));
    branch.checkpoint(&mut store).unwrap();
    let grandchild = branch.fork(&mut store, "branch-b").unwrap();
    assert!(branch.shares_prefix_with(&grandchild));
    assert_eq!(branch.breakpoint_sha256(), grandchild.breakpoint_sha256());
    assert_eq!(
        branch.rendered().as_bytes(),
        grandchild.rendered().as_bytes()
    );
    assert_eq!(grandchild.cost().novelty_bytes, 0);
}
