import { WidgetType, type EditorView } from "@codemirror/view";
import {
  parseTable,
  serializeTable,
  parseTSV,
  type TableModel,
} from "../tableParse";
import { applyTableOp, type TableOp } from "../tableOps";
import { readTableFocus } from "../tableFocus";
import { openTableMenu, type MenuAction } from "./tableMenu";
import { dropTarget } from "./dragIndex";

export class EditableTableWidget extends WidgetType {
  constructor(
    readonly md: string,
    readonly from: number,
    readonly to: number,
    readonly onCommit: (from: number, to: number, md: string) => void,
  ) {
    super();
  }

  /** True while a structural op dispatches + re-mounts. The dispatch tears down
   *  this widget's DOM, blurring the focused cell and firing a spurious
   *  `focusout`; this flag tells the commit handler to ignore that. */
  private applying = false;

  eq(other: EditableTableWidget): boolean {
    return (
      other.md === this.md && other.from === this.from && other.to === this.to
    );
  }
  ignoreEvent(): boolean {
    return true;
  }

  toDOM(view: EditorView): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "cm-lp-table-edit";
    const table = document.createElement("table");
    table.className = "cm-lp-table editing";
    table.setAttribute("role", "grid");
    wrap.appendChild(table);
    const model = parseTable(this.md);
    this.render(view, wrap, table, model);

    // Commit in-progress cell text once focus leaves the whole table.
    wrap.addEventListener("focusout", (e) => {
      if (this.applying) return; // structural re-render in progress, not a real blur
      const next = e.relatedTarget as Node | null;
      if (next && wrap.contains(next)) return; // moving between cells
      const md = serializeTable(this.readModel(table, model));
      if (md !== serializeTable(parseTable(this.md)))
        this.onCommit(this.from, this.to, md);
    });

    // After layout: align grips to their rows/columns, then restore focus to the
    // cell recorded before the last structural op (else the first cell on entry).
    requestAnimationFrame(() => {
      this.positionGrips(wrap, table);
      const target = readTableFocus(view.state);
      if (target && target.pos === this.from) {
        this.focusCell(table, target.row, target.col);
      } else {
        wrap.querySelector<HTMLElement>("th, td")?.focus();
      }
    });
    return wrap;
  }

  /** Align each grip to its column (left+width) / row (top+height) by measuring
   *  the laid-out cells. Grips are wrapper-absolute; CSS sets the cross-axis edge. */
  private positionGrips(wrap: HTMLElement, table: HTMLTableElement): void {
    const base = wrap.getBoundingClientRect();
    const colGrips = wrap.querySelectorAll<HTMLElement>(".cm-lp-col-grip");
    table.querySelectorAll<HTMLElement>("thead th").forEach((th, i) => {
      const r = th.getBoundingClientRect();
      const g = colGrips[i];
      if (!g) return;
      g.style.left = `${r.left - base.left}px`;
      g.style.width = `${r.width}px`;
    });
    const rowGrips = wrap.querySelectorAll<HTMLElement>(".cm-lp-row-grip");
    table.querySelectorAll<HTMLElement>("tbody tr").forEach((tr, i) => {
      const r = tr.getBoundingClientRect();
      const g = rowGrips[i];
      if (!g) return;
      g.style.top = `${r.top - base.top}px`;
      g.style.height = `${r.height}px`;
    });
  }

  /** Read the live model from the DOM, preserving alignment from `base` (alignment
   *  has no DOM affordance this round). Reads only direct text nodes so grip/control
   *  buttons inside cells are excluded from the committed text. */
  protected readModel(table: HTMLTableElement, base: TableModel): TableModel {
    const text = (cell: Element): string =>
      [...cell.childNodes]
        .filter((n) => n.nodeType === Node.TEXT_NODE)
        .map((n) => n.textContent ?? "")
        .join("")
        .trim();
    const header = [...table.querySelectorAll("thead th")].map(text);
    const rows = [...table.querySelectorAll("tbody tr")].map((tr) =>
      [...tr.querySelectorAll("td")].map(text),
    );
    const align = header.map((_, i) => base.align[i] ?? "none");
    return { header, rows, align };
  }

  /** Capture current DOM text (incl. uncommitted typing), apply a structural op,
   *  and dispatch ONE transaction via the shared spine. */
  private op(
    view: EditorView,
    table: HTMLTableElement,
    base: TableModel,
    build: (m: TableModel) => TableOp,
    focus: { row: number; col: number },
  ): void {
    const current = this.readModel(table, base);
    const md = serializeTable(current);
    this.applying = true;
    applyTableOp(view, this.from, this.to, build(current), focus, md);
    // If a dispatch happened, the old DOM (and this instance) is discarded after
    // the spurious focusout; if it was a no-op, clear the guard so real blurs commit.
    requestAnimationFrame(() => {
      this.applying = false;
    });
  }

  protected render(
    view: EditorView,
    wrap: HTMLElement,
    table: HTMLTableElement,
    model: TableModel,
  ): void {
    table.textContent = "";
    wrap
      .querySelectorAll(
        ".cm-lp-row-grip, .cm-lp-col-grip, .cm-lp-add-col, .cm-lp-add-row",
      )
      .forEach((n) => n.remove());

    const thead = table.createTHead();
    const hr = thead.insertRow();
    model.header.forEach((h, ci) => {
      const th = document.createElement("th");
      th.contentEditable = "plaintext-only";
      th.setAttribute("role", "gridcell");
      th.textContent = h;
      this.cellKeys(table, th);
      hr.appendChild(th);
      wrap.appendChild(
        this.grip(
          "cm-lp-col-grip",
          "col",
          ci,
          () => this.columnActions(view, table, model, ci),
          (to) =>
            this.op(
              view,
              table,
              model,
              () => ({ kind: "moveColumn", from: ci, to }),
              { row: -1, col: to },
            ),
          () =>
            [...table.querySelectorAll("thead th")].map((el) => {
              const r = el.getBoundingClientRect();
              return r.left + r.width / 2;
            }),
        ),
      );
    });

    const tbody = table.createTBody();
    model.rows.forEach((row, ri) => {
      const tr = tbody.insertRow();
      row.forEach((c, ci) => {
        const td = tr.insertCell();
        td.contentEditable = "plaintext-only";
        td.setAttribute("role", "gridcell");
        td.textContent = c;
        this.cellKeys(table, td);
        this.cellPaste(view, table, model, td, ri, ci);
        this.cellContext(view, table, model, td, ri, ci);
      });
      wrap.appendChild(
        this.grip(
          "cm-lp-row-grip",
          "row",
          ri,
          () => this.rowActions(view, table, model, ri),
          (to) =>
            this.op(
              view,
              table,
              model,
              () => ({ kind: "moveRow", from: ri, to }),
              { row: to, col: 0 },
            ),
          () =>
            [...table.querySelectorAll("tbody tr")].map((el) => {
              const r = el.getBoundingClientRect();
              return r.top + r.height / 2;
            }),
        ),
      );
    });

    wrap.appendChild(
      this.ctl("cm-lp-add-col", "+", () =>
        this.op(
          view,
          table,
          model,
          (m) => ({ kind: "insertColumn", index: m.header.length }),
          { row: -1, col: model.header.length },
        ),
      ),
    );
    wrap.appendChild(
      this.ctl("cm-lp-add-row", "+", () =>
        this.op(
          view,
          table,
          model,
          (m) => ({ kind: "insertRow", index: m.rows.length }),
          { row: model.rows.length, col: 0 },
        ),
      ),
    );
  }

  private rowActions(
    view: EditorView,
    table: HTMLTableElement,
    model: TableModel,
    ri: number,
  ): MenuAction[] {
    const run = (
      build: (m: TableModel) => TableOp,
      focus: { row: number; col: number },
    ) => this.op(view, table, model, build, focus);
    return [
      {
        label: "Insert row above",
        run: () =>
          run(() => ({ kind: "insertRow", index: ri }), { row: ri, col: 0 }),
      },
      {
        label: "Insert row below",
        run: () =>
          run(() => ({ kind: "insertRow", index: ri + 1 }), {
            row: ri + 1,
            col: 0,
          }),
      },
      {
        label: "Move row up",
        run: () =>
          run(() => ({ kind: "moveRow", from: ri, to: ri - 1 }), {
            row: ri - 1,
            col: 0,
          }),
      },
      {
        label: "Move row down",
        run: () =>
          run(() => ({ kind: "moveRow", from: ri, to: ri + 1 }), {
            row: ri + 1,
            col: 0,
          }),
      },
      {
        label: "Delete row",
        danger: true,
        run: () =>
          run(() => ({ kind: "removeRow", index: ri }), {
            row: Math.max(0, ri - 1),
            col: 0,
          }),
      },
    ];
  }

  private columnActions(
    view: EditorView,
    table: HTMLTableElement,
    model: TableModel,
    ci: number,
  ): MenuAction[] {
    const run = (
      build: (m: TableModel) => TableOp,
      focus: { row: number; col: number },
    ) => this.op(view, table, model, build, focus);
    return [
      {
        label: "Insert column left",
        run: () =>
          run(() => ({ kind: "insertColumn", index: ci }), {
            row: -1,
            col: ci,
          }),
      },
      {
        label: "Insert column right",
        run: () =>
          run(() => ({ kind: "insertColumn", index: ci + 1 }), {
            row: -1,
            col: ci + 1,
          }),
      },
      {
        label: "Move column left",
        run: () =>
          run(() => ({ kind: "moveColumn", from: ci, to: ci - 1 }), {
            row: -1,
            col: ci - 1,
          }),
      },
      {
        label: "Move column right",
        run: () =>
          run(() => ({ kind: "moveColumn", from: ci, to: ci + 1 }), {
            row: -1,
            col: ci + 1,
          }),
      },
      {
        label: "Delete column",
        danger: true,
        run: () =>
          run(() => ({ kind: "removeColumn", index: ci }), {
            row: -1,
            col: Math.max(0, ci - 1),
          }),
      },
    ];
  }

  /** Focus the cell at (row,col); row -1 = header. */
  private focusCell(table: HTMLTableElement, row: number, col: number): void {
    const target =
      row < 0
        ? table.querySelectorAll<HTMLElement>("thead th")[col]
        : table
            .querySelectorAll("tbody tr")
            [row]?.querySelectorAll<HTMLElement>("td")[col];
    (target ?? table.querySelector<HTMLElement>("th, td"))?.focus();
  }

  /** A grip button. Click opens the action menu; dragging reorders its row/column.
   *  The `moved` guard suppresses the click that fires after a drag. */
  private grip(
    cls: string,
    axis: "row" | "col",
    index: number,
    actions: () => MenuAction[],
    onMove: (to: number) => void,
    centers: () => number[],
  ): HTMLElement {
    const g = document.createElement("button");
    g.type = "button";
    g.className = cls;
    g.contentEditable = "false";
    g.setAttribute("aria-haspopup", "menu");
    g.textContent = "⠿"; // braille dots = grip handle
    let down = false;
    let moved = false;
    g.addEventListener("mousedown", (e) => e.preventDefault()); // keep caret
    g.addEventListener("pointerdown", (e) => {
      down = true;
      moved = false;
      g.setPointerCapture?.(e.pointerId);
    });
    g.addEventListener("pointermove", () => {
      if (down) moved = true;
    });
    g.addEventListener("pointerup", (e) => {
      if (down && moved) {
        const list = centers();
        const p = axis === "row" ? e.clientY : e.clientX;
        const to = dropTarget(p, list, index);
        if (to !== index) onMove(to);
      }
      down = false;
    });
    g.addEventListener("pointercancel", () => {
      down = false;
      moved = false;
    });
    g.addEventListener("click", (e) => {
      if (moved) {
        moved = false;
        e.preventDefault();
        e.stopPropagation();
        return; // this click is the tail of a drag — ignore
      }
      openTableMenu(g, actions());
    });
    return g;
  }

  /** Tab / Shift-Tab between cells; Enter → cell below; Esc → leave (commit). */
  private cellKeys(table: HTMLElement, cell: HTMLElement): void {
    cell.addEventListener("keydown", (e) => {
      const cells = [...table.querySelectorAll<HTMLElement>("th, td")];
      const i = cells.indexOf(cell);
      const cols = table.querySelectorAll("thead th").length || 1;
      if (e.key === "Tab") {
        e.preventDefault();
        cells[i + (e.shiftKey ? -1 : 1)]?.focus();
      } else if (e.key === "Enter") {
        e.preventDefault();
        cells[i + cols]?.focus();
      } else if (e.key === "Escape") {
        e.preventDefault();
        cell.blur();
      } else if ((e.ctrlKey || e.metaKey) && (e.key === "a" || e.key === "A")) {
        // Keep select-all scoped to this cell (CodeMirror binds Mod-a to a
        // whole-document selectAll). Stop propagation but not default.
        e.stopPropagation();
      }
    });
  }

  /** Paste handler: a multi-cell TSV block spills across cells (auto-growing the
   *  table) anchored at this body cell; a single value falls through to the
   *  browser's default text insert. */
  private cellPaste(
    view: EditorView,
    table: HTMLTableElement,
    model: TableModel,
    cell: HTMLElement,
    row: number,
    col: number,
  ): void {
    cell.addEventListener("paste", (e) => {
      const text =
        (e as ClipboardEvent).clipboardData?.getData("text/plain") ?? "";
      const block = parseTSV(text);
      const multi = block.length > 1 || (block[0]?.length ?? 0) > 1;
      if (!multi) return; // single value → let the browser insert text normally
      e.preventDefault();
      this.op(
        view,
        table,
        model,
        () => ({ kind: "paste", atRow: row, atCol: col, block }),
        { row, col },
      );
    });
  }

  /** Right-click menu: clipboard ops + this cell's row and column actions. */
  private cellContext(
    view: EditorView,
    table: HTMLTableElement,
    model: TableModel,
    cell: HTMLElement,
    row: number,
    col: number,
  ): void {
    cell.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const actions: MenuAction[] = [
        { label: "Cut", run: () => void document.execCommand("cut") },
        { label: "Copy", run: () => void document.execCommand("copy") },
        {
          label: "Paste",
          run: () =>
            void this.pasteFromClipboard(view, table, model, cell, row, col),
        },
        ...this.rowActions(view, table, model, row),
        ...this.columnActions(view, table, model, col),
      ];
      openTableMenu(cell, actions);
    });
  }

  /** Paste via the async Clipboard API: a multi-cell TSV payload spills through
   *  the paste op; a single value is inserted into the focused cell. Best effort
   *  — silently no-ops when the clipboard is unavailable or denied. */
  private async pasteFromClipboard(
    view: EditorView,
    table: HTMLTableElement,
    model: TableModel,
    cell: HTMLElement,
    row: number,
    col: number,
  ): Promise<void> {
    try {
      const text = await navigator.clipboard.readText();
      const block = parseTSV(text);
      if (block.length > 1 || (block[0]?.length ?? 0) > 1) {
        this.op(
          view,
          table,
          model,
          () => ({ kind: "paste", atRow: row, atCol: col, block }),
          { row, col },
        );
      } else if (text) {
        cell.focus();
        document.execCommand("insertText", false, text);
      }
    } catch {
      // clipboard unavailable / permission denied — no-op
    }
  }

  /** A non-editable control button that doesn't steal the contenteditable caret. */
  private ctl(cls: string, label: string, onClick: () => void): HTMLElement {
    const b = document.createElement("button");
    b.className = cls;
    b.type = "button";
    b.textContent = label;
    b.contentEditable = "false";
    b.addEventListener("mousedown", (e) => {
      e.preventDefault();
      onClick();
    });
    return b;
  }
}
