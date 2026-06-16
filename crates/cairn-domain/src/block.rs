//! Splitting a note's markdown into blocks and joining blocks back to
//! canonical markdown. Pure string work, no I/O.

/// The kind of a markdown block. Used as metadata on a CRDT block; it does
/// not affect convergence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Frontmatter,
    Heading,
    Paragraph,
    ListItem,
    CodeFence,
    BlockQuote,
    Table,
    ThematicBreak,
}

/// One parsed block: its kind and its exact source text (surrounding blank
/// lines trimmed off).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub text: String,
}

/// Classify a single block's text by its first line.
fn classify(text: &str) -> BlockKind {
    let first = text.lines().next().unwrap_or("");
    let t = first.trim_start();
    if first.starts_with("```") || first.starts_with("~~~") {
        BlockKind::CodeFence
    } else if t == "---" || t == "***" || t == "___" {
        BlockKind::ThematicBreak
    } else if t.starts_with("# ")
        || t.starts_with("## ")
        || t.starts_with("### ")
        || t.starts_with("#### ")
        || t.starts_with("##### ")
        || t.starts_with("###### ")
    {
        BlockKind::Heading
    } else if is_list_item(first) {
        BlockKind::ListItem
    } else if t.starts_with('>') {
        BlockKind::BlockQuote
    } else if t.starts_with('|') {
        BlockKind::Table
    } else {
        BlockKind::Paragraph
    }
}

/// A list-item line: `- `, `* `, `+ `, or `<digits>. ` (ordered).
fn is_list_item(line: &str) -> bool {
    let t = line.trim_start();
    if t.strip_prefix("- ")
        .or(t.strip_prefix("* "))
        .or(t.strip_prefix("+ "))
        .is_some()
    {
        return true; // marker alone still a list item
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    !digits.is_empty() && t[digits.len()..].starts_with(". ")
}

/// Split a note's markdown into blocks. Boundary = blank line. A fenced code
/// block is one atomic block. A run of consecutive list-item lines splits into
/// one block per item.
#[must_use]
pub fn parse_blocks(src: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        // Skip blank separator lines.
        if lines[i].trim().is_empty() {
            i += 1;
            continue;
        }
        // Fenced code block: consume until the closing fence.
        if lines[i].starts_with("```") || lines[i].starts_with("~~~") {
            let fence = &lines[i][..3];
            let start = i;
            i += 1;
            while i < lines.len() && !lines[i].starts_with(fence) {
                i += 1;
            }
            if i < lines.len() {
                i += 1; // include closing fence
            }
            let text = lines[start..i].join("\n");
            blocks.push(Block {
                kind: BlockKind::CodeFence,
                text,
            });
            continue;
        }
        // Gather a chunk: consecutive non-blank lines.
        let start = i;
        while i < lines.len() && !lines[i].trim().is_empty() {
            // A code fence inside a chunk starts a new block — stop here.
            if i > start && (lines[i].starts_with("```") || lines[i].starts_with("~~~")) {
                break;
            }
            i += 1;
        }
        let chunk = &lines[start..i];
        // If every line in the chunk is a list item, split one block per line.
        if chunk.iter().all(|l| is_list_item(l)) {
            for line in chunk {
                blocks.push(Block {
                    kind: BlockKind::ListItem,
                    text: (*line).to_string(),
                });
            }
        } else {
            let text = chunk.join("\n");
            let kind = classify(&text);
            blocks.push(Block { kind, text });
        }
    }
    blocks
}

/// Join block source texts into canonical markdown: one blank line between
/// blocks, a single trailing newline, no leading/trailing blank lines. Adjacent
/// list-item blocks are separated by a single newline (not a blank line), since
/// `parse_blocks` splits one list into per-item blocks and a blank line between
/// them would reflow the list. This is the normalization the round-trip
/// property is defined against.
#[must_use]
pub fn join_blocks(texts: &[String]) -> String {
    if texts.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, text) in texts.iter().enumerate() {
        if i > 0 {
            // Consecutive list items belong to one list: single newline, no
            // blank line. Every other boundary gets a blank line.
            let glue = if is_list_item(&texts[i - 1]) && is_list_item(text) {
                "\n"
            } else {
                "\n\n"
            };
            out.push_str(glue);
        }
        out.push_str(text);
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_paragraph_is_one_block() {
        let b = parse_blocks("The review went well.\nWe hit our targets.");
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].kind, BlockKind::Paragraph);
        assert_eq!(b[0].text, "The review went well.\nWe hit our targets.");
    }

    #[test]
    fn blank_line_separates_blocks() {
        let b = parse_blocks("First para.\n\nSecond para.");
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].text, "First para.");
        assert_eq!(b[1].text, "Second para.");
    }

    #[test]
    fn each_list_item_is_a_block() {
        let b = parse_blocks("- call Bob\n- ship v1");
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].kind, BlockKind::ListItem);
        assert_eq!(b[0].text, "- call Bob");
        assert_eq!(b[1].text, "- ship v1");
    }

    #[test]
    fn code_fence_is_one_atomic_block() {
        let src = "```rust\nfn main() {}\n\nlet x = 1;\n```";
        let b = parse_blocks(src);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].kind, BlockKind::CodeFence);
        assert_eq!(b[0].text, src);
    }

    #[test]
    fn heading_and_thematic_break_classified() {
        let b = parse_blocks("# Title\n\n---");
        assert_eq!(b[0].kind, BlockKind::Heading);
        assert_eq!(b[1].kind, BlockKind::ThematicBreak);
    }

    #[test]
    fn join_separates_with_one_blank_line_and_trailing_newline() {
        let joined = join_blocks(&["# Title".into(), "Body para.".into()]);
        assert_eq!(joined, "# Title\n\nBody para.\n");
    }

    #[test]
    fn round_trip_normalized_markdown_is_identity() {
        let src = "# Title\n\nFirst para.\n\n- a\n- b\n";
        let blocks = parse_blocks(src);
        let texts: Vec<String> = blocks.iter().map(|b| b.text.clone()).collect();
        assert_eq!(join_blocks(&texts), src);
    }
}
