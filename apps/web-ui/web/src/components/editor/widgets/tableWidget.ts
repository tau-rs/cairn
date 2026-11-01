import { WidgetType } from "@codemirror/view";
import { parseTable } from "../tableParse";

export class TableWidget extends WidgetType {
  constructor(
    readonly md: string,
    readonly from: number,
    readonly onEnterEdit: (from: number) => void,
  ) {
    super();
  }
  eq(other: TableWidget): boolean {
    return other.md === this.md && other.from === this.from;
  }
  toDOM(): HTMLElement {
    const { header, rows, align } = parseTable(this.md);
    const table = document.createElement("table");
    table.className = "cm-lp-table";
    const css = (i: number): string =>
      align[i] && align[i] !== "none" ? align[i] : "";
    const thead = table.createTHead();
    const hr = thead.insertRow();
    header.forEach((h, i) => {
      const th = document.createElement("th");
      th.textContent = h;
      if (css(i)) th.style.textAlign = css(i);
      hr.appendChild(th);
    });
    const tbody = table.createTBody();
    for (const r of rows) {
      const tr = tbody.insertRow();
      r.forEach((c, i) => {
        const td = tr.insertCell();
        td.textContent = c;
        if (css(i)) td.style.textAlign = css(i);
      });
    }
    table.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.onEnterEdit(this.from);
    });
    return table;
  }
  ignoreEvent(): boolean {
    return false;
  }
}
