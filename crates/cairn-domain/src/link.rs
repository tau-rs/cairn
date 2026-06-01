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
}
