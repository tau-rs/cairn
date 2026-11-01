import { describe, it, expect, vi } from "vitest";
import { EditorState, EditorSelection } from "@codemirror/state";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorView, type DecorationSet } from "@codemirror/view";
import { buildLivePreviewDecorations, livePreview } from "./livePreview";

const opts = {
  resolve: (t: string) => (t === "ideas" ? "ideas.md" : null),
  onOpenNote: vi.fn(),
  onToggleCheckbox: vi.fn(),
  resolveImage: (src: string) => ({
    kind: "ready" as const,
    url: "resolved:" + src,
  }),
  onEditImage: vi.fn(),
  onLoadImage: vi.fn(),
  onEnterTableEdit: vi.fn(),
  onCommitTable: vi.fn(),
};

interface Deco {
  from: number;
  to: number;
  class?: string;
  hidden: boolean;
  widget: boolean;
}

function decos(doc: string, cursor: number): Deco[] {
  const state = EditorState.create({
    doc,
    selection: EditorSelection.cursor(cursor),
    extensions: [markdown({ base: markdownLanguage })],
  });
  const set: DecorationSet = buildLivePreviewDecorations(
    state,
    opts,
  ).decorations;
  const out: Deco[] = [];
  set.between(0, doc.length, (from, to, value) => {
    const spec = value.spec as { class?: string; widget?: unknown };
    out.push({
      from,
      to,
      class: spec.class,
      widget: spec.widget != null,
      hidden: spec.class == null && spec.widget == null,
    });
  });
  return out;
}

function atomicRanges(
  doc: string,
  cursor: number,
): { from: number; to: number }[] {
  const state = EditorState.create({
    doc,
    selection: EditorSelection.cursor(cursor),
    extensions: [markdown({ base: markdownLanguage })],
  });
  const set = buildLivePreviewDecorations(state, opts).atomic;
  const out: { from: number; to: number }[] = [];
  set.between(0, doc.length, (from, to) => {
    out.push({ from, to });
  });
  return out;
}

describe("atomic ranges (widgets only)", () => {
  it("marks a widget (image) range as atomic", () => {
    const doc = "see ![a](x.png) end";
    const at = doc.indexOf("![");
    expect(atomicRanges(doc, 0).some((r) => r.from === at)).toBe(true);
  });
  it("marks a table widget range as atomic", () => {
    const doc = "intro\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\nend";
    const at = doc.indexOf("| A");
    expect(atomicRanges(doc, 0).some((r) => r.from === at)).toBe(true);
  });
  it("does NOT mark a blockquote's hidden > marker as atomic", () => {
    const doc = "> quoted\n\nbody";
    // cursor off the quote so the > is hidden (a plain replace) — but not atomic
    const ranges = atomicRanges(doc, doc.indexOf("body"));
    expect(ranges.some((r) => r.from === 0)).toBe(false);
  });
  it("does NOT mark a heading's hidden # marker as atomic", () => {
    const doc = "# Title\n\nbody";
    const ranges = atomicRanges(doc, doc.indexOf("body"));
    expect(ranges.some((r) => r.from === 0)).toBe(false);
  });
});

describe("buildLivePreviewDecorations", () => {
  it("styles a heading and hides the # marker when the cursor is elsewhere", () => {
    const doc = "# Title\n\nbody";
    const ds = decos(doc, doc.indexOf("body"));
    expect(ds.some((d) => d.class === "cm-lp-h1")).toBe(true);
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(true);
  });
  it("reveals the # marker when the cursor is on the heading line", () => {
    const ds = decos("# Title\n\nbody", 3);
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(false);
  });
  it("hides ** markers around bold off-cursor", () => {
    const ds = decos("a **b** c", 0);
    expect(ds.some((d) => d.class === "cm-lp-strong")).toBe(true);
    expect(ds.filter((d) => d.hidden).length).toBeGreaterThanOrEqual(2);
  });
  it("renders a resolved [[wikilink]] as a widget off-cursor", () => {
    const ds = decos("see [[ideas]] end", 0);
    expect(ds.some((d) => d.widget)).toBe(true);
  });
  it("reveals raw [[wikilink]] when the cursor is inside it", () => {
    const ds = decos("see [[ideas]] end", 7);
    expect(ds.some((d) => d.widget)).toBe(false);
  });
  it("marks bullet list lines and replaces the bullet marker off-cursor", () => {
    const doc = "- alpha\n- beta";
    const ds = decos(doc, doc.length); // cursor on 2nd line end
    // first line's marker (positions 0..1 = "- ") is replaced by a bullet widget
    expect(ds.some((d) => d.widget && d.from === 0)).toBe(true);
    // a list-item line class is applied
    expect(ds.some((d) => d.class === "cm-lp-li")).toBe(true);
  });
  it("reveals the raw bullet marker when the cursor is on that item", () => {
    const ds = decos("- alpha\n- beta", 2); // cursor inside first item
    expect(ds.some((d) => d.widget && d.from === 0)).toBe(false);
  });
  it("keeps the number marker on ordered lists (no bullet widget)", () => {
    const doc = "1. one\n2. two";
    const ds = decos(doc, doc.length);
    // ordered markers are not replaced with a bullet widget
    expect(ds.some((d) => d.widget && d.from === 0)).toBe(false);
    expect(ds.some((d) => d.class === "cm-lp-li")).toBe(true);
  });
  it("styles a blockquote line and hides the > marker off-cursor", () => {
    const doc = "> quoted\n\nbody";
    const ds = decos(doc, doc.indexOf("body"));
    expect(ds.some((d) => d.class === "cm-lp-quote")).toBe(true);
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(true);
  });
  it("reveals the > marker when the cursor is in the quote", () => {
    const ds = decos("> quoted\n\nbody", 2);
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(false);
  });
  it("replaces a horizontal rule with a widget off-cursor", () => {
    const doc = "a\n\n---\n\nb";
    const hrPos = doc.indexOf("---");
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.widget && d.from === hrPos)).toBe(true);
  });
  it("reveals the raw rule when the cursor is on it", () => {
    const doc = "a\n\n---\n\nb";
    const hrPos = doc.indexOf("---");
    const ds = decos(doc, hrPos + 1);
    expect(ds.some((d) => d.widget && d.from === hrPos)).toBe(false);
  });
  it("styles fenced-code lines and hides the fence lines off-cursor", () => {
    const doc = "text\n\n```js\nconst x = 1;\n```\n\nmore";
    const fence = doc.indexOf("```");
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.class === "cm-lp-codeblock")).toBe(true);
    // opening fence line is hidden
    expect(ds.some((d) => d.hidden && d.from === fence)).toBe(true);
    const closingFence = doc.lastIndexOf("```");
    expect(ds.some((d) => d.hidden && d.from === closingFence)).toBe(true);
  });
  it("reveals the fences when the cursor is inside the code block", () => {
    const doc = "text\n\n```js\nconst x = 1;\n```\n\nmore";
    const fence = doc.indexOf("```");
    const ds = decos(doc, doc.indexOf("const"));
    expect(ds.some((d) => d.hidden && d.from === fence)).toBe(false);
    const closingFence = doc.lastIndexOf("```");
    expect(ds.some((d) => d.hidden && d.from === closingFence)).toBe(false);
  });
  it("renders a task checkbox as a widget off-cursor", () => {
    const doc = "- [ ] todo item";
    const ds = decos(doc, doc.length); // cursor at end, off the marker
    const open = doc.indexOf("[");
    expect(ds.some((d) => d.widget && d.from === open)).toBe(true);
  });
  it("reveals the raw [ ] marker when the cursor is on the task", () => {
    const doc = "- [ ] todo item";
    const open = doc.indexOf("[");
    const ds = decos(doc, open + 1);
    expect(ds.some((d) => d.widget && d.from === open)).toBe(false);
  });
  it("hides the list bullet for a task item (checkbox only, no bullet)", () => {
    const doc = "- [ ] todo item";
    const ds = decos(doc, doc.length); // off the marker
    // the "- " marker is hidden (a plain replace, not a widget) at the line start
    expect(ds.some((d) => d.from === 0 && d.hidden)).toBe(true);
    expect(ds.some((d) => d.from === 0 && d.widget)).toBe(false);
    // the checkbox widget renders at the "[" position
    const open = doc.indexOf("[");
    expect(ds.some((d) => d.widget && d.from === open)).toBe(true);
  });
  it("replaces an image with a widget off-cursor", () => {
    const doc = "see ![logo](img/logo.png) here";
    const at = doc.indexOf("![");
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.widget && d.from === at)).toBe(true);
  });
  it("reveals the raw image markdown when the cursor is on it", () => {
    const doc = "see ![logo](img/logo.png) here";
    const at = doc.indexOf("![");
    const ds = decos(doc, at + 2);
    expect(ds.some((d) => d.widget && d.from === at)).toBe(false);
  });
  it("still renders an image widget at the match position off-cursor", () => {
    const doc = "see ![logo](img/logo.png) end";
    const at = doc.indexOf("![");
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.widget && d.from === at)).toBe(true);
  });
  it("does not emit inline marks inside an image's alt text (no overlap with the widget)", () => {
    const doc = "see ![*a*](x.png) end";
    const ds = decos(doc, 0); // off-cursor: image is a widget
    const at = doc.indexOf("![");
    // the image renders as a single widget...
    expect(ds.some((d) => d.widget && d.from === at)).toBe(true);
    // ...and NO emphasis mark or hide decoration lands inside the image range
    const imgEnd = at + "![*a*](x.png)".length;
    const inside = ds.filter(
      (d) => d.from >= at && d.to <= imgEnd && !d.widget,
    );
    expect(inside.length).toBe(0);
  });
  it("replaces a table with a single block widget off-cursor", () => {
    const doc = "intro\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\nend";
    const at = doc.indexOf("| A");
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.widget && d.from === at)).toBe(true);
  });
  it("swaps to the editable table widget when the cursor is inside it", () => {
    const doc = "intro\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\nend";
    const at = doc.indexOf("| A");
    const ds = decos(doc, doc.indexOf("| 1"));
    // Touched → still a single block widget at the table start (the editable
    // one), not raw pipes — local-DOM editing avoids a CM rebuild mid-edit.
    expect(ds.some((d) => d.widget && d.from === at)).toBe(true);
  });
  it("does not emit cell decorations inside a table block (table widget owns the range)", () => {
    const doc = "intro\n\n| A | [[ideas]] |\n|---|---|\n| 1 | 2 |\n\nend";
    const tableAt = doc.indexOf("| A");
    const ds = decos(doc, 0);
    // exactly one widget covers the table range start; no wikilink widget inside
    expect(ds.some((d) => d.widget && d.from === tableAt)).toBe(true);
    const wikilinkAt = doc.indexOf("[[ideas]]");
    expect(ds.some((d) => d.widget && d.from === wikilinkAt)).toBe(false);
  });
  it("mounts an EditorView with a table without throwing (block decoration via StateField)", () => {
    const doc = "intro\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\nend";
    const state = EditorState.create({
      doc,
      extensions: [markdown({ base: markdownLanguage }), livePreview(opts)],
    });
    expect(() => {
      const view = new EditorView({ state });
      view.destroy();
    }).not.toThrow();
  });

  it("styles frontmatter content lines and hides the --- fences off-cursor", () => {
    const doc = "---\ntitle: x\ntags: a\n---\n\nbody";
    const ds = decos(doc, doc.indexOf("body"));
    // content lines carry the frontmatter class
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter"))).toBe(true);
    // opening fence (offset 0) and closing fence are hidden
    const closeFence = doc.indexOf("---", 3);
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(true);
    expect(ds.some((d) => d.hidden && d.from === closeFence)).toBe(true);
    // first/last marker classes are present
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter-first"))).toBe(
      true,
    );
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter-last"))).toBe(
      true,
    );
  });

  it("does not emit a horizontal-rule widget for the opening fence", () => {
    const doc = "---\ntitle: x\n---\n\nbody";
    const ds = decos(doc, doc.indexOf("body"));
    // without the suppression guard the first --- parses as an HR widget at 0
    expect(ds.some((d) => d.widget && d.from === 0)).toBe(false);
  });

  it("reveals raw frontmatter when the cursor is inside the block", () => {
    const doc = "---\ntitle: x\n---\n\nbody";
    const ds = decos(doc, doc.indexOf("title"));
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter"))).toBe(false);
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(false);
  });

  it("treats --- that is not on line 1 as a horizontal rule, not frontmatter", () => {
    const doc = "intro\n\n---\n\nbody";
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter"))).toBe(false);
    const hrPos = doc.indexOf("---");
    expect(ds.some((d) => d.widget && d.from === hrPos)).toBe(true);
  });

  it("ignores unclosed frontmatter (no second ---)", () => {
    const doc = "---\ntitle: x\n\nbody";
    const ds = decos(doc, doc.indexOf("body"));
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter"))).toBe(false);
  });

  it("handles empty frontmatter (--- immediately followed by ---)", () => {
    const doc = "---\n---\n\nbody";
    const ds = decos(doc, doc.indexOf("body"));
    // both fence lines are hidden and carry the first/last marker classes
    const closeFence = doc.indexOf("---", 3);
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(true);
    expect(ds.some((d) => d.hidden && d.from === closeFence)).toBe(true);
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter-first"))).toBe(
      true,
    );
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter-last"))).toBe(
      true,
    );
    // no HR widget leaks at offset 0
    expect(ds.some((d) => d.widget && d.from === 0)).toBe(false);
  });

  it("suppresses a [[wikilink]] inside the frontmatter block", () => {
    const doc = "---\nlinks: [[ideas]]\n---\n\nbody";
    const ds = decos(doc, doc.indexOf("body"));
    // no wikilink widget should render for the [[ideas]] inside frontmatter
    const wikilinkPos = doc.indexOf("[[ideas]]");
    expect(ds.some((d) => d.widget && d.from === wikilinkPos)).toBe(false);
  });
});
