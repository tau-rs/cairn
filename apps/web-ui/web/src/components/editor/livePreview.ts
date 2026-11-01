import { syntaxTree } from "@codemirror/language";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";
import {
  StateField,
  type EditorState,
  type Extension,
  type Range,
} from "@codemirror/state";
import { WikilinkWidget } from "./wikilinkWidget";
import { BulletWidget } from "./widgets/bulletWidget";
import { HrWidget } from "./widgets/hrWidget";
import { TaskCheckboxWidget } from "./widgets/taskCheckboxWidget";
import { ImageWidget } from "./widgets/imageWidget";
import type { ResolvedImage } from "./imageResolver";
import { TableWidget } from "./widgets/tableWidget";
import { EditableTableWidget } from "./widgets/editableTableWidget";
import { tableFocus } from "./tableFocus";

export interface LivePreviewOptions {
  resolve: (target: string) => string | null;
  onOpenNote: (path: string) => void;
  onToggleCheckbox: (bracketOpen: number) => void;
  resolveImage: (src: string) => ResolvedImage;
  onEditImage: (from: number) => void;
  onLoadImage: (src: string) => void;
  onEnterTableEdit: (from: number) => void;
  onCommitTable: (from: number, to: number, md: string) => void;
}

const HEADING_CLASS: Record<string, string> = {
  ATXHeading1: "cm-lp-h1",
  ATXHeading2: "cm-lp-h2",
  ATXHeading3: "cm-lp-h3",
  ATXHeading4: "cm-lp-h4",
  ATXHeading5: "cm-lp-h5",
  ATXHeading6: "cm-lp-h6",
};
const INLINE_CLASS: Record<string, string> = {
  StrongEmphasis: "cm-lp-strong",
  Emphasis: "cm-lp-em",
  Strikethrough: "cm-lp-strike",
  InlineCode: "cm-lp-code",
};
const MARK_CHILD: Record<string, string> = {
  StrongEmphasis: "EmphasisMark",
  Emphasis: "EmphasisMark",
  Strikethrough: "StrikethroughMark",
  InlineCode: "CodeMark",
};

const WIKILINK = /\[\[([^\]]+?)\]\]/g;

function selectionTouches(
  state: EditorState,
  from: number,
  to: number,
): boolean {
  return state.selection.ranges.some((r) => r.from <= to && r.to >= from);
}

export function lineRange(state: EditorState, from: number, to: number) {
  return { start: state.doc.lineAt(from).from, end: state.doc.lineAt(to).to };
}

function isInsideCode(state: EditorState, pos: number): boolean {
  const node = syntaxTree(state).resolveInner(pos, 1);
  for (let n: typeof node | null = node; n; n = n.parent) {
    if (/Code/.test(n.name)) return true;
  }
  return false;
}

/** Walk the ancestor chain at `pos` looking for a node named `nodeName`. */
function isInsidePos(
  state: EditorState,
  pos: number,
  nodeName: string,
): boolean {
  const node = syntaxTree(state).resolveInner(pos, 1);
  for (let n: typeof node | null = node; n; n = n.parent) {
    if (n.name === nodeName) return true;
  }
  return false;
}

type SyntaxNode = ReturnType<typeof syntaxTree>["topNode"];

function isInsideImage(node: { node: SyntaxNode }): boolean {
  for (let n: SyntaxNode | null = node.node.parent; n; n = n.parent) {
    if (n.name === "Image") return true;
  }
  return false;
}

/** Walk a node's ancestors looking for an enclosing `Table` (for inline
 *  branches inside table cells, whose range the block table widget owns). */
function isInsideTable(node: { node: SyntaxNode }): boolean {
  for (let n: SyntaxNode | null = node.node.parent; n; n = n.parent) {
    if (n.name === "Table") return true;
  }
  return false;
}

/** Detect a leading YAML frontmatter block: line 1 is exactly `---`, with a
 *  later line that is exactly `---` closing it. Returns the block's document
 *  span and fence line numbers, or null when there is no closing fence (or the
 *  doc does not open with `---`). The markdown parser mis-parses this region, so
 *  it is detected by text-scan and its stray nodes are suppressed below. */
function frontmatterRange(
  state: EditorState,
): { start: number; end: number; openLine: number; closeLine: number } | null {
  const first = state.doc.line(1);
  if (first.text !== "---") return null;
  for (let n = 2; n <= state.doc.lines; n++) {
    const line = state.doc.line(n);
    if (line.text === "---") {
      return { start: first.from, end: line.to, openLine: 1, closeLine: n };
    }
  }
  return null;
}

/** PURE: build the live-preview decoration set for a given editor state. */
export function buildLivePreviewDecorations(
  state: EditorState,
  opts: LivePreviewOptions,
): { decorations: DecorationSet; atomic: DecorationSet } {
  const decos: Range<Decoration>[] = [];
  const tree = syntaxTree(state);
  const fm = frontmatterRange(state);

  tree.iterate({
    enter: (node) => {
      const { name, from, to } = node;
      if (fm && from < fm.end) return; // suppress parser nodes inside frontmatter
      if (HEADING_CLASS[name]) {
        decos.push(
          Decoration.mark({ class: HEADING_CLASS[name] }).range(from, to),
        );
        if (!selectionTouches(state, from, to)) {
          const mark = node.node.getChild("HeaderMark");
          if (mark) {
            // hide the #'s and the following space
            const end = Math.min(mark.to + 1, to);
            decos.push(Decoration.replace({}).range(mark.from, end));
          }
        }
      } else if (INLINE_CLASS[name]) {
        if (isInsideImage(node)) return;
        if (isInsideTable(node)) return;
        decos.push(
          Decoration.mark({ class: INLINE_CLASS[name] }).range(from, to),
        );
        if (!selectionTouches(state, from, to)) {
          for (const m of node.node.getChildren(MARK_CHILD[name])) {
            decos.push(Decoration.replace({}).range(m.from, m.to));
          }
        }
      } else if (name === "Link") {
        if (isInsideImage(node)) return;
        if (isInsideTable(node)) return;
        // A `[[wikilink]]` parses its inner `[wikilink]` as a Link node; skip it
        // so the wikilink widget (below) owns that range — otherwise we'd emit a
        // stray cm-lp-link class and overlapping replace decorations.
        if (state.doc.sliceString(from - 1, from) === "[") return;
        decos.push(Decoration.mark({ class: "cm-lp-link" }).range(from, to));
        if (!selectionTouches(state, from, to)) {
          // hide the leading `[` and the trailing `](url)`, keep the link text
          const openBracket = node.node.getChild("LinkMark"); // first [
          if (openBracket)
            decos.push(
              Decoration.replace({}).range(openBracket.from, openBracket.to),
            );
          const closeStart = findCloseBracket(
            state,
            openBracket ? openBracket.to : from,
            to,
          );
          if (closeStart != null)
            decos.push(Decoration.replace({}).range(closeStart, to));
        }
      } else if (name === "ListItem") {
        // Style each line of the item; replace a bullet marker with a • widget.
        const firstLine = state.doc.lineAt(from);
        // indent every line of the item (continuation lines included)
        for (let pos = from; pos <= to; pos = state.doc.lineAt(pos).to + 1) {
          const ln = state.doc.lineAt(pos);
          decos.push(Decoration.line({ class: "cm-lp-li" }).range(ln.from));
          if (ln.to >= to) break;
        }
        const taskMatch = /^(\s*[-*+]\s+)(\[[ xX]\])/.exec(firstLine.text);
        const mark = node.node.getChild("ListMark");

        if (taskMatch && mark) {
          // Task item: hide the list marker entirely and render only the checkbox.
          const open = firstLine.from + taskMatch[1].length; // index of "["
          const close = open + 3; // covers "[ ]"
          if (!selectionTouches(state, open, close)) {
            // hide the "- " marker (keep any leading indent before mark.from)
            decos.push(Decoration.replace({}).range(mark.from, open));
            const checked = /[xX]/.test(
              firstLine.text[open - firstLine.from + 1],
            );
            decos.push(
              Decoration.replace({
                widget: new TaskCheckboxWidget(
                  checked,
                  open,
                  opts.onToggleCheckbox,
                ),
              }).range(open, close),
            );
          }
        } else if (mark) {
          // Non-task list item: bullet marker handling (unordered only).
          const markText = state.doc.sliceString(mark.from, mark.to);
          const isBullet = /^[-*+]$/.test(markText);
          if (
            isBullet &&
            !selectionTouches(state, firstLine.from, firstLine.to)
          ) {
            const end = Math.min(mark.to + 1, firstLine.to);
            decos.push(
              Decoration.replace({ widget: new BulletWidget() }).range(
                mark.from,
                end,
              ),
            );
          }
        }
      } else if (name === "Blockquote") {
        // Class every line of the quote; hide each line's "> " mark off-cursor.
        const touched = selectionTouches(state, from, to);
        let pos = from;
        while (pos <= to) {
          const line = state.doc.lineAt(pos);
          decos.push(
            Decoration.line({ class: "cm-lp-quote" }).range(line.from),
          );
          if (!touched) {
            const match = /^(\s*>\s?)/.exec(line.text);
            if (match) {
              decos.push(
                Decoration.replace({}).range(
                  line.from,
                  line.from + match[1].length,
                ),
              );
            }
          }
          if (line.to >= to) break;
          pos = line.to + 1;
        }
      } else if (name === "HorizontalRule") {
        if (!selectionTouches(state, from, to)) {
          decos.push(
            Decoration.replace({ widget: new HrWidget() }).range(from, to),
          );
        }
      } else if (name === "FencedCode") {
        const lr = lineRange(state, from, to);
        const touched = selectionTouches(state, lr.start, lr.end);
        const firstLine = state.doc.lineAt(from).number;
        const lastLine = state.doc.lineAt(to).number;
        for (let n = firstLine; n <= lastLine; n++) {
          const line = state.doc.line(n);
          decos.push(
            Decoration.line({ class: "cm-lp-codeblock" }).range(line.from),
          );
          const isFence = /^\s*```/.test(line.text);
          if (isFence && !touched && line.length > 0) {
            decos.push(Decoration.replace({}).range(line.from, line.to));
          }
        }
      } else if (name === "Table") {
        const start = state.doc.lineAt(from).from;
        const end = state.doc.lineAt(to).to;
        const md = state.doc.sliceString(start, end);
        if (selectionTouches(state, start, end)) {
          decos.push(
            Decoration.replace({
              widget: new EditableTableWidget(
                md,
                start,
                end,
                opts.onCommitTable,
              ),
              block: true,
            }).range(start, end),
          );
        } else {
          decos.push(
            Decoration.replace({
              widget: new TableWidget(md, start, opts.onEnterTableEdit),
              block: true,
            }).range(start, end),
          );
        }
      }
    },
  });

  // Wikilinks: the markdown parser ignores [[...]], so scan the text.
  const text = state.doc.toString();
  WIKILINK.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = WIKILINK.exec(text)) !== null) {
    const from = m.index;
    const to = from + m[0].length;
    if (isInsideCode(state, from)) continue;
    if (isInsidePos(state, from, "Table")) continue;
    if (fm && from < fm.end) continue;
    const inner = m[1];
    const target = inner.split("|")[0].trim();
    if (!target) continue;
    if (selectionTouches(state, from, to)) continue; // reveal raw
    const alias = inner.includes("|")
      ? inner.slice(inner.indexOf("|") + 1).trim()
      : target;
    const path = opts.resolve(target);
    decos.push(
      Decoration.replace({
        widget: new WikilinkWidget(alias, path, opts.onOpenNote),
      }).range(from, to),
    );
  }

  // Images: scan the text (like wikilinks) so nesting/aliases don't trip up a
  // single-token tree lookup. Skip images inside code, reveal raw on cursor.
  // Intentionally does NOT handle spaces-in-path, `"title"` suffixes, or
  // bracketed alt text — those forms stay raw.
  const IMAGE = /!\[([^\]]*)\]\(([^)\s]+)\)/g;
  IMAGE.lastIndex = 0;
  let im: RegExpExecArray | null;
  while ((im = IMAGE.exec(text)) !== null) {
    const from = im.index;
    const to = from + im[0].length;
    if (isInsideCode(state, from)) continue;
    if (isInsidePos(state, from, "Table")) continue;
    if (fm && from < fm.end) continue;
    if (selectionTouches(state, from, to)) continue;
    const alt = im[1];
    const image = opts.resolveImage(im[2]);
    const line = state.doc.lineAt(from);
    const block = line.text.trim() === im[0];
    decos.push(
      Decoration.replace({
        widget: new ImageWidget(
          image,
          alt,
          block,
          from,
          opts.onEditImage,
          opts.onLoadImage,
        ),
      }).range(from, to),
    );
  }

  // Frontmatter: a styled box off-cursor (line classes + hidden `---` fences),
  // raw when the cursor is inside. The parser mis-reads this region, so it is
  // handled here by text-scan and suppressed in the tree iterate above.
  if (fm && !selectionTouches(state, fm.start, fm.end)) {
    for (let n = fm.openLine; n <= fm.closeLine; n++) {
      const line = state.doc.line(n);
      let cls = "cm-lp-frontmatter";
      if (n === fm.openLine) cls += " cm-lp-frontmatter-first";
      if (n === fm.closeLine) cls += " cm-lp-frontmatter-last";
      decos.push(Decoration.line({ class: cls }).range(line.from));
      if ((n === fm.openLine || n === fm.closeLine) && line.length > 0) {
        decos.push(Decoration.replace({}).range(line.from, line.to));
      }
    }
  }

  const decorations = Decoration.set(decos, /* sort */ true);
  // Only widget-bearing decorations are atomic (so the caret skips rendered
  // widgets). Plain marker-hide replaces (#, **, > , ```) must NOT be atomic, or
  // a click near a hidden line-start marker gets pushed out of the element.
  const atomicDecos = decos.filter(
    (r) => (r.value.spec as { widget?: unknown }).widget != null,
  );
  const atomic = Decoration.set(atomicDecos, /* sort */ true);
  return { decorations, atomic };
}

/** Find the closing `]` position of a link, between `searchFrom` and `to`. */
function findCloseBracket(
  state: EditorState,
  searchFrom: number,
  to: number,
): number | null {
  const slice = state.doc.sliceString(searchFrom, to);
  const idx = slice.indexOf("]");
  return idx === -1 ? null : searchFrom + idx;
}

/** The live-preview CodeMirror extension. */
export function livePreview(opts: LivePreviewOptions): Extension {
  const field = StateField.define<{
    decorations: DecorationSet;
    atomic: DecorationSet;
  }>({
    create(state) {
      return buildLivePreviewDecorations(state, opts);
    },
    update(value, tr) {
      if (tr.docChanged || tr.selection) {
        return buildLivePreviewDecorations(tr.state, opts);
      }
      return value;
    },
    provide: (f) => [
      EditorView.decorations.from(f, (v) => v.decorations),
      EditorView.atomicRanges.of(
        (view) => view.state.field(f)?.atomic ?? Decoration.none,
      ),
    ],
  });
  return [field, tableFocus];
}
