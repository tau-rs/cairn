export type Align = "none" | "left" | "center" | "right";

export interface TableModel {
  header: string[];
  rows: string[][];
  align: Align[];
}

// Split a table row on UNescaped pipes, drop outer pipes, unescape \| → | then \\ → \.
const cells = (line: string): string[] =>
  line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split(/(?<!\\)\|/)
    .map((c) => c.trim().replace(/\\\|/g, "|").replace(/\\\\/g, "\\"));

/** Read one delimiter cell (e.g. `:--`, `:-:`, `--:`, `---`) into an Align. */
function parseAlign(cell: string): Align {
  const t = cell.trim();
  const left = t.startsWith(":");
  const right = t.endsWith(":");
  if (left && right) return "center";
  if (right) return "right";
  if (left) return "left";
  return "none";
}

/** Parse a GFM pipe table's source into header + body rows + per-column alignment
 *  (read from line 2, the delimiter row). */
// NOTE: assumes line 2 is the GFM delimiter row; malformed input (no delimiter) may drop body rows.
export function parseTable(md: string): TableModel {
  const lines = md
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
  if (lines.length === 0) return { header: [], rows: [], align: [] };
  const header = cells(lines[0]);
  const rawAlign = lines.length > 1 ? cells(lines[1]).map(parseAlign) : [];
  const align: Align[] = header.map((_, i) => rawAlign[i] ?? "none");
  const rows = lines.slice(2).map(cells);
  return { header, rows, align };
}

const escapeCell = (s: string): string =>
  s.trim().replace(/\\/g, "\\\\").replace(/\|/g, "\\|");

/** A padded delimiter cell of width `w` for the given alignment. */
function marker(a: Align, w: number): string {
  if (a === "left") return ":" + "-".repeat(Math.max(1, w - 1));
  if (a === "right") return "-".repeat(Math.max(1, w - 1)) + ":";
  if (a === "center") return ":" + "-".repeat(Math.max(1, w - 2)) + ":";
  return "-".repeat(w);
}

/** Serialize a model to prettified GFM: columns padded to equal width, alignment
 *  markers preserved. Minimum column width is 3 so every marker form fits. */
export function serializeTable(m: TableModel): string {
  const cols = m.header.length;
  const escHeader = m.header.map(escapeCell);
  const escRows = m.rows.map((r) =>
    Array.from({ length: cols }, (_, c) => escapeCell(r[c] ?? "")),
  );
  const widths = Array.from({ length: cols }, (_, c) =>
    Math.max(3, escHeader[c].length, ...escRows.map((r) => r[c].length)),
  );
  const fmtRow = (cs: string[]): string =>
    `| ${cs.map((s, c) => s.padEnd(widths[c])).join(" | ")} |`;
  const delim = `| ${widths
    .map((w, c) => marker(m.align[c] ?? "none", w))
    .join(" | ")} |`;
  return [fmtRow(escHeader), delim, ...escRows.map(fmtRow)].join("\n");
}

const clamp = (n: number, lo: number, hi: number): number =>
  Math.max(lo, Math.min(hi, n));
const blankRow = (n: number): string[] => Array.from({ length: n }, () => "");
const splice = <T>(arr: T[], from: number, to: number): T[] => {
  const a = [...arr];
  const [x] = a.splice(from, 1);
  a.splice(to, 0, x);
  return a;
};

/** Insert a blank row at `index` (clamped to [0, rows.length]). */
export function insertRow(m: TableModel, index: number): TableModel {
  const i = clamp(index, 0, m.rows.length);
  return {
    header: [...m.header],
    rows: [
      ...m.rows.slice(0, i),
      blankRow(m.header.length),
      ...m.rows.slice(i),
    ],
    align: [...m.align],
  };
}

/** Append a blank row. */
export function addRow(m: TableModel): TableModel {
  return insertRow(m, m.rows.length);
}

/** Remove a body row; keeps at least one body row. */
export function removeRow(m: TableModel, index: number): TableModel {
  if (m.rows.length <= 1) return m;
  return {
    header: [...m.header],
    rows: m.rows.filter((_, i) => i !== index),
    align: [...m.align],
  };
}

/** Move a body row from one index to another. No-op if out of range or unchanged. */
export function moveRow(m: TableModel, from: number, to: number): TableModel {
  if (
    from === to ||
    from < 0 ||
    to < 0 ||
    from >= m.rows.length ||
    to >= m.rows.length
  )
    return m;
  return {
    header: [...m.header],
    rows: splice(m.rows, from, to),
    align: [...m.align],
  };
}

const insertAt = <T>(arr: T[], i: number, v: T): T[] => [
  ...arr.slice(0, i),
  v,
  ...arr.slice(i),
];

/** Insert a blank column at `index` (clamped), with the given alignment. */
export function insertColumn(
  m: TableModel,
  index: number,
  align: Align = "none",
): TableModel {
  const i = clamp(index, 0, m.header.length);
  return {
    header: insertAt(m.header, i, ""),
    rows: m.rows.map((r) => insertAt(r, i, "")),
    align: insertAt(m.align, i, align),
  };
}

/** Append a blank column. */
export function addColumn(m: TableModel): TableModel {
  return insertColumn(m, m.header.length);
}

/** Remove a column (and its alignment); keeps at least one column. */
export function removeColumn(m: TableModel, index: number): TableModel {
  if (m.header.length <= 1) return m;
  const del = <T>(arr: T[]): T[] => arr.filter((_, i) => i !== index);
  return { header: del(m.header), rows: m.rows.map(del), align: del(m.align) };
}

/** Move a column (header, every cell, and align in lockstep). No-op if unchanged. */
export function moveColumn(
  m: TableModel,
  from: number,
  to: number,
): TableModel {
  if (
    from === to ||
    from < 0 ||
    to < 0 ||
    from >= m.header.length ||
    to >= m.header.length
  )
    return m;
  return {
    header: splice(m.header, from, to),
    rows: m.rows.map((r) => splice(r, from, to)),
    align: splice(m.align, from, to),
  };
}

/** Parse a spreadsheet clipboard payload (TSV): tab = column, newline = row.
 *  Normalizes CRLF and ignores trailing blank lines. */
export function parseTSV(text: string): string[][] {
  const norm = text.replace(/\r\n?/g, "\n").replace(/\n+$/, "");
  if (norm === "") return [];
  return norm.split("\n").map((line) => line.split("\t"));
}

/** Paste a 2-D block of cells into the body, anchored at (atRow, atCol) in body
 *  coordinates. Auto-grows rows and columns so the whole block fits. */
export function pasteBlock(
  m: TableModel,
  atRow: number,
  atCol: number,
  block: string[][],
): TableModel {
  if (block.length === 0) return m;
  const blockCols = Math.max(...block.map((r) => r.length));
  let model = m;
  while (model.header.length < atCol + blockCols) model = addColumn(model);
  while (model.rows.length < atRow + block.length) model = addRow(model);
  const rows = model.rows.map((r) => [...r]);
  block.forEach((br, ri) =>
    br.forEach((val, ci) => {
      const R = atRow + ri;
      const C = atCol + ci;
      if (R >= 0 && R < rows.length && C >= 0 && C < rows[R].length)
        rows[R][C] = val;
    }),
  );
  return { header: [...model.header], rows, align: [...model.align] };
}
