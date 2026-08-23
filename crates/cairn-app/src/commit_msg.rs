//! Deterministic commit-subject generation from a [`DiffSummary`]. Pure — no
//! I/O — so history stays reproducible from a known diff.

use cairn_ports::{ChangeOp, DiffSummary, NoteChange};

/// Render the counts suffix, eliding zero sides. Empty when both are zero.
fn counts(added: u32, removed: u32) -> String {
    match (added, removed) {
        (0, 0) => String::new(),
        (a, 0) => format!(" (+{a} words)"),
        (0, r) => format!(" (−{r} words)"),
        (a, r) => format!(" (+{a}/−{r} words)"),
    }
}

/// Deterministic commit subject for `summary`. Never includes timestamps.
#[must_use]
pub fn commit_message(summary: &DiffSummary) -> String {
    match summary.changes.as_slice() {
        [] => "Checkpoint".to_string(),
        [c] => single(c),
        many => {
            let titles: Vec<String> = many
                .iter()
                .take(3)
                .map(|c| format!("\"{}\"", c.title))
                .collect();
            let ellipsis = if many.len() > 3 { "…" } else { "" };
            format!(
                "Update {} notes: {}{ellipsis}",
                many.len(),
                titles.join(", ")
            )
        }
    }
}

fn single(c: &NoteChange) -> String {
    let heading = c
        .heading
        .as_deref()
        .map(|h| format!(" § {h}"))
        .unwrap_or_default();
    match &c.op {
        ChangeOp::Add => format!("Add \"{}\"{}", c.title, counts(c.words_added, 0)),
        ChangeOp::Edit => format!(
            "Edit \"{}\"{heading}{}",
            c.title,
            counts(c.words_added, c.words_removed)
        ),
        ChangeOp::Delete => format!("Delete \"{}\"", c.title),
        ChangeOp::Rename { from } => {
            let old = std::path::Path::new(from)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| from.clone());
            format!(
                "Rename \"{old}\" → \"{}\"{}",
                c.title,
                counts(c.words_added, c.words_removed)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(op: ChangeOp, title: &str, added: u32, removed: u32) -> NoteChange {
        NoteChange {
            path: format!("{title}.md"),
            op,
            title: title.into(),
            heading: None,
            words_added: added,
            words_removed: removed,
        }
    }

    fn summary(changes: Vec<NoteChange>) -> DiffSummary {
        let (a, r) = changes
            .iter()
            .fold((0, 0), |(a, r), c| (a + c.words_added, r + c.words_removed));
        DiffSummary {
            changes,
            words_added: a,
            words_removed: r,
        }
    }

    #[test]
    fn single_edit_with_heading_and_both_counts() {
        let mut c = change(ChangeOp::Edit, "Q3 Roadmap", 112, 8);
        c.heading = Some("Goals".into());
        assert_eq!(
            commit_message(&summary(vec![c])),
            "Edit \"Q3 Roadmap\" § Goals (+112/−8 words)"
        );
    }

    #[test]
    fn single_edit_elides_zero_sides() {
        let c = change(ChangeOp::Edit, "A", 112, 0);
        assert_eq!(commit_message(&summary(vec![c])), "Edit \"A\" (+112 words)");
        let c = change(ChangeOp::Edit, "A", 0, 8);
        assert_eq!(commit_message(&summary(vec![c])), "Edit \"A\" (−8 words)");
        let c = change(ChangeOp::Edit, "A", 0, 0);
        assert_eq!(commit_message(&summary(vec![c])), "Edit \"A\"");
    }

    #[test]
    fn single_add_and_delete() {
        let c = change(ChangeOp::Add, "Meeting 2026-08-22", 540, 0);
        assert_eq!(
            commit_message(&summary(vec![c])),
            "Add \"Meeting 2026-08-22\" (+540 words)"
        );
        // Delete never shows counts.
        let c = change(ChangeOp::Delete, "Scratch", 0, 300);
        assert_eq!(commit_message(&summary(vec![c])), "Delete \"Scratch\"");
    }

    #[test]
    fn single_rename_shows_old_stem_and_counts_only_if_content_changed() {
        let mut c = change(
            ChangeOp::Rename {
                from: "old-name.md".into(),
            },
            "New Name",
            0,
            0,
        );
        assert_eq!(
            commit_message(&summary(vec![c.clone()])),
            "Rename \"old-name\" → \"New Name\""
        );
        c.words_added = 4;
        assert_eq!(
            commit_message(&summary(vec![c])),
            "Rename \"old-name\" → \"New Name\" (+4 words)"
        );
    }

    #[test]
    fn multi_note_rollup_caps_titles_at_three() {
        let s = summary(vec![
            change(ChangeOp::Edit, "A", 1, 0),
            change(ChangeOp::Add, "B", 2, 0),
            change(ChangeOp::Edit, "C", 3, 0),
        ]);
        assert_eq!(commit_message(&s), "Update 3 notes: \"A\", \"B\", \"C\"");
        let s = summary(vec![
            change(ChangeOp::Edit, "A", 1, 0),
            change(ChangeOp::Edit, "B", 1, 0),
            change(ChangeOp::Edit, "C", 1, 0),
            change(ChangeOp::Edit, "D", 1, 0),
        ]);
        assert_eq!(commit_message(&s), "Update 4 notes: \"A\", \"B\", \"C\"…");
    }

    #[test]
    fn empty_summary_is_checkpoint() {
        assert_eq!(commit_message(&DiffSummary::default()), "Checkpoint");
    }
}
