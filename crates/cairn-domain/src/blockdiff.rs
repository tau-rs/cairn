//! Pure LCS sequence diff over block texts: the edit script that
//! `BlockDoc::fold_foreign` maps onto live block IDs. No CRDT knowledge, no I/O.

/// One step aligning a `base` block sequence to a `foreign` one. Indices are
/// into the respective `parse_blocks` outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiffStep {
    /// `base[bi]` and `foreign[fi]` are byte-identical — keep the block.
    Keep { bi: usize, fi: usize },
    /// `base[bi]` has no counterpart — removed on disk.
    Delete { bi: usize },
    /// `foreign[fi]` has no counterpart — added on disk.
    Insert { fi: usize },
}

/// Longest-common-subsequence edit script between two block-text sequences.
/// Deletes/Inserts are emitted in source order; a substitution surfaces as a
/// `Delete` immediately followed by an `Insert` (the caller pairs them into a
/// content update). O(n·m) time/space — block counts per note are small.
pub(crate) fn lcs_edit_script(base: &[String], foreign: &[String]) -> Vec<DiffStep> {
    let (n, m) = (base.len(), foreign.len());
    // dp[i][j] = LCS length of base[i..] and foreign[j..].
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if base[i] == foreign[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut steps = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if base[i] == foreign[j] {
            steps.push(DiffStep::Keep { bi: i, fi: j });
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            steps.push(DiffStep::Delete { bi: i });
            i += 1;
        } else {
            steps.push(DiffStep::Insert { fi: j });
            j += 1;
        }
    }
    while i < n {
        steps.push(DiffStep::Delete { bi: i });
        i += 1;
    }
    while j < m {
        steps.push(DiffStep::Insert { fi: j });
        j += 1;
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn identical_sequences_are_all_keeps() {
        let s = lcs_edit_script(&v(&["a", "b"]), &v(&["a", "b"]));
        assert_eq!(
            s,
            vec![
                DiffStep::Keep { bi: 0, fi: 0 },
                DiffStep::Keep { bi: 1, fi: 1 }
            ]
        );
    }

    #[test]
    fn pure_insertion_in_the_middle() {
        let s = lcs_edit_script(&v(&["a", "c"]), &v(&["a", "b", "c"]));
        assert_eq!(
            s,
            vec![
                DiffStep::Keep { bi: 0, fi: 0 },
                DiffStep::Insert { fi: 1 },
                DiffStep::Keep { bi: 1, fi: 2 },
            ]
        );
    }

    #[test]
    fn pure_deletion() {
        let s = lcs_edit_script(&v(&["a", "b", "c"]), &v(&["a", "c"]));
        assert_eq!(
            s,
            vec![
                DiffStep::Keep { bi: 0, fi: 0 },
                DiffStep::Delete { bi: 1 },
                DiffStep::Keep { bi: 2, fi: 1 },
            ]
        );
    }

    #[test]
    fn substitution_is_delete_then_insert() {
        // "b" -> "B": no common block, so delete b then insert B, framed by keeps.
        let s = lcs_edit_script(&v(&["a", "b", "c"]), &v(&["a", "B", "c"]));
        assert_eq!(
            s,
            vec![
                DiffStep::Keep { bi: 0, fi: 0 },
                DiffStep::Delete { bi: 1 },
                DiffStep::Insert { fi: 1 },
                DiffStep::Keep { bi: 2, fi: 2 },
            ]
        );
    }

    #[test]
    fn empty_base_is_all_inserts() {
        let s = lcs_edit_script(&v(&[]), &v(&["x", "y"]));
        assert_eq!(
            s,
            vec![DiffStep::Insert { fi: 0 }, DiffStep::Insert { fi: 1 }]
        );
    }

    #[test]
    fn empty_foreign_is_all_deletes() {
        let s = lcs_edit_script(&v(&["x", "y"]), &v(&[]));
        assert_eq!(
            s,
            vec![DiffStep::Delete { bi: 0 }, DiffStep::Delete { bi: 1 }]
        );
    }
}
