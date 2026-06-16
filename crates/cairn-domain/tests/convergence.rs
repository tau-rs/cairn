//! Property tests for BlockDoc convergence (commutativity, associativity,
//! idempotence) and markdown round-trip. See ADR-0011.

use cairn_domain::block::{join_blocks, parse_blocks};
use cairn_domain::crdt::{Author, BlockDoc, BlockOp};
use cairn_domain::{BlockId, Edit};
use proptest::prelude::*;

/// A small, normalized-markdown generator: 1–6 blocks, each a heading,
/// paragraph, or list item, joined by single blank lines + trailing newline.
fn normalized_markdown() -> impl Strategy<Value = String> {
    let block = prop_oneof![
        "# [A-Za-z ]{1,12}".prop_map(|s| s.trim_end().to_string()),
        "[A-Za-z][A-Za-z ]{0,20}".prop_map(|s| s.trim_end().to_string()),
        "- [A-Za-z][A-Za-z ]{0,12}".prop_map(|s| s.trim_end().to_string()),
    ]
    .prop_filter("non-empty", |s| !s.trim().is_empty());
    prop::collection::vec(block, 1..6).prop_map(|texts| join_blocks(&texts))
}

proptest! {
    /// Round-trip: parse then join is the identity on normalized markdown.
    #[test]
    fn round_trip_is_identity(src in normalized_markdown()) {
        let texts: Vec<String> = parse_blocks(&src).iter().map(|b| b.text.clone()).collect();
        prop_assert_eq!(join_blocks(&texts), src);
    }

    /// from_markdown -> materialize is the identity on normalized markdown.
    #[test]
    fn doc_round_trip_is_identity(src in normalized_markdown()) {
        let doc = BlockDoc::from_markdown(1, &src);
        prop_assert_eq!(doc.materialize(), src);
    }
}

/// Generate a pool of ops by having two replicas each make a few local edits
/// against the same seed, collecting the emitted ops.
fn op_pool(seed: &str) -> Vec<BlockOp> {
    let mut ops = Vec::new();
    let mut a = BlockDoc::from_markdown(1, seed);
    let mut b = BlockDoc::from_markdown(2, seed);
    // Replicas 1 and 2 seed identical structure but with distinct birth-replica
    // block ids, so b's ops here land on ids absent from a fresh replica-1 doc
    // (they no-op on merge). This pool therefore exercises ordering/idempotence
    // of insert+update; same-block concurrent *content* edits are stressed
    // directly by `same_block_content_edits_converge` below.
    let a_ids: Vec<BlockId> = a.block_ids_in_order();
    let b_ids: Vec<BlockId> = b.block_ids_in_order();
    if let Some(&id) = a_ids.first() {
        ops.extend(a.apply_local(Edit::UpdateText {
            id,
            text: "A-edit".into(),
            author: Author::Human,
        }));
        ops.extend(a.apply_local(Edit::InsertAfter {
            after: Some(id),
            kind: cairn_domain::BlockKind::Paragraph,
            text: "A-new".into(),
            author: Author::Human,
        }));
    }
    if let Some(&id) = b_ids.first() {
        ops.extend(b.apply_local(Edit::UpdateText {
            id,
            text: "B-edit".into(),
            author: Author::Agent,
        }));
        ops.extend(b.apply_local(Edit::Remove { id }));
    }
    ops
}

proptest! {
    /// Convergence: applying the same op pool in any permutation, with one op
    /// duplicated, yields identical materialized markdown on every replica.
    #[test]
    fn replicas_converge_under_any_order(perm in any::<u64>()) {
        let seed = "seed one\n\nseed two\n";
        let mut pool = op_pool(seed);
        if let Some(first) = pool.first().cloned() {
            pool.push(first); // duplication ⇒ exercises idempotence
        }

        // Deterministic shuffle of `pool` driven by `perm` (no external rng).
        let mut order: Vec<usize> = (0..pool.len()).collect();
        let mut s = perm | 1;
        for i in (1..order.len()).rev() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = (s >> 33) as usize % (i + 1);
            order.swap(i, j);
        }

        // Replica X: apply in shuffled order. Replica Y: apply in pool order.
        let mut x = BlockDoc::from_markdown(1, seed);
        let mut y = BlockDoc::from_markdown(1, seed);
        for &k in &order {
            x.merge(pool[k].clone());
        }
        for op in &pool {
            y.merge(op.clone());
        }
        prop_assert_eq!(x.materialize(), y.materialize());
    }
}

proptest! {
    /// Same-block convergence: many SetContent ops on ONE shared block id, with a
    /// small Lamport range to force frequent (author, lamport) ties, converge
    /// regardless of application order — directly stressing the LWW total order
    /// and its text tiebreak. Closes the gap left by `replicas_converge_under_
    /// any_order` (whose cross-replica ops no-op on absent ids).
    #[test]
    fn same_block_content_edits_converge(
        ops in prop::collection::vec((any::<bool>(), 0u64..6, "[A-C]{1,3}"), 1..8),
    ) {
        let seed = "shared\n";
        // First block minted by from_markdown(1, _) has this id.
        let id = BlockId { replica: 1, counter: 0 };
        let pool: Vec<BlockOp> = ops
            .iter()
            .map(|(human, lamport, text)| BlockOp::SetContent {
                id,
                text: text.clone(),
                lamport: *lamport,
                author: if *human { Author::Human } else { Author::Agent },
            })
            .collect();

        let mut x = BlockDoc::from_markdown(1, seed);
        let mut y = BlockDoc::from_markdown(1, seed);
        // x: forward order + a duplicate (idempotence). y: reverse order.
        for op in &pool {
            x.merge(op.clone());
        }
        x.merge(pool[0].clone());
        for op in pool.iter().rev() {
            y.merge(op.clone());
        }
        prop_assert_eq!(x.materialize(), y.materialize());
    }
}
