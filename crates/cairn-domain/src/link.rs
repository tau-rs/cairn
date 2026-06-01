//! Extraction of `[[wikilink]]` targets from markdown body text.

/// A link target referenced by a note via `[[target]]` syntax.
/// The target is the raw text inside the brackets, trimmed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinkTarget(pub String);

/// Extract all `[[...]]` link targets from `body`, in order of appearance,
/// including duplicates. An alias form `[[target|alias]]` yields `target`.
#[must_use]
pub fn extract_links(body: &str) -> Vec<LinkTarget> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(close) = body[i + 2..].find("]]") {
                let inner = &body[i + 2..i + 2 + close];
                let target = inner.split('|').next().unwrap_or("").trim();
                if !target.is_empty() {
                    out.push(LinkTarget(target.to_string()));
                }
                i = i + 2 + close + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Rewrite `[[from]]` -> `[[to]]` and `[[from|alias]]` -> `[[to|alias]]` in
/// `content`, matching link targets by exact trimmed text. Non-matching links
/// and all other text are left verbatim. Operates on raw content (no frontmatter
/// parsing).
#[must_use]
pub fn rewrite_link_target(content: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < content.len() {
        if i + 1 < bytes.len() && bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(close) = content[i + 2..].find("]]") {
                let span_end = i + 2 + close + 2;
                let inner = &content[i + 2..i + 2 + close];
                let (target, alias) = match inner.split_once('|') {
                    Some((t, a)) => (t, Some(a)),
                    None => (inner, None),
                };
                if target.trim() == from {
                    out.push_str("[[");
                    out.push_str(to);
                    if let Some(a) = alias {
                        out.push('|');
                        out.push_str(a);
                    }
                    out.push_str("]]");
                } else {
                    out.push_str(&content[i..span_end]);
                }
                i = span_end;
                continue;
            }
        }
        let ch = content[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_and_aliased_links() {
        let links = extract_links("see [[Alpha]] and [[Beta|the second]] end");
        assert_eq!(
            links,
            vec![LinkTarget("Alpha".into()), LinkTarget("Beta".into())]
        );
    }

    #[test]
    fn ignores_unclosed_and_empty() {
        assert_eq!(extract_links("[[ ]] and [[unclosed"), Vec::new());
    }

    #[test]
    fn rewrite_plain_and_aliased_and_leaves_others() {
        assert_eq!(
            rewrite_link_target("see [[a]] end", "a", "b"),
            "see [[b]] end"
        );
        assert_eq!(rewrite_link_target("[[a|the A]]", "a", "b"), "[[b|the A]]");
        assert_eq!(
            rewrite_link_target("[[c]] and [[a]]", "a", "b"),
            "[[c]] and [[b]]"
        );
        assert_eq!(rewrite_link_target("no links", "a", "b"), "no links");
        assert_eq!(rewrite_link_target("[[a]] [[a]]", "a", "b"), "[[b]] [[b]]");
        // UTF-8 safety: multibyte content around brackets must not panic and is
        // preserved byte-for-byte.
        assert_eq!(
            rewrite_link_target("héllo [[a]] wörld", "a", "b"),
            "héllo [[b]] wörld"
        );
    }
}
