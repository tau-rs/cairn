/** Extract `[[target]]` / `[[target|alias]]` targets from body text,
 *  in order, with duplicates. Mirrors cairn-domain `extract_links`. */
export function extractLinks(body: string): string[] {
  const out: string[] = [];
  let i = 0;
  while (i + 1 < body.length) {
    if (body[i] === "[" && body[i + 1] === "[") {
      const close = body.indexOf("]]", i + 2);
      if (close !== -1) {
        const inner = body.slice(i + 2, close);
        const target = inner.split("|")[0].trim();
        if (target.length > 0) out.push(target);
        i = close + 2;
        continue;
      }
    }
    i += 1;
  }
  return out;
}

/** File stem: filename without directory or `.md`. Mirrors cairn-domain `stem`. */
export function stem(path: string): string {
  const afterSlash = path.split("/").pop() ?? path;
  return afterSlash.endsWith(".md") ? afterSlash.slice(0, -3) : afterSlash;
}
