/** Tags from a note's frontmatter `tags:` key (inline `[a, b]` / `a, b`, or a
 *  `- item` block list). Lowercased, deduped, order preserved. Mirrors the engine,
 *  which reads tags from frontmatter only (no inline `#tag`). */
export function extractTags(markdown: string): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  const add = (raw: string) => {
    const t = raw
      .trim()
      .replace(/^['"]|['"]$/g, "")
      .toLowerCase();
    if (t && !seen.has(t)) {
      seen.add(t);
      out.push(t);
    }
  };

  if (markdown.startsWith("---\n")) {
    const end = markdown.indexOf("\n---", 4);
    if (end !== -1) {
      const fm = markdown.slice(4, end);
      const lines = fm.split("\n");
      const i = lines.findIndex((l) => /^tags:/.test(l));
      if (i !== -1) {
        const inline = lines[i].replace(/^tags:\s*/, "").trim();
        if (inline.startsWith("[")) {
          inline
            .replace(/^\[|\]$/g, "")
            .split(",")
            .forEach(add);
        } else if (inline) {
          inline.split(",").forEach(add);
        } else {
          for (let j = i + 1; j < lines.length; j++) {
            const m = /^\s*-\s*(.+)$/.exec(lines[j]);
            if (m) add(m[1]);
            else break;
          }
        }
      }
    }
  }
  return out;
}
