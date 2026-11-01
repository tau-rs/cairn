import { WidgetType } from "@codemirror/view";

/** A clickable checkbox; on mousedown it asks the host to toggle the source. */
export class TaskCheckboxWidget extends WidgetType {
  constructor(
    readonly checked: boolean,
    readonly bracketOpen: number,
    readonly onToggle: (bracketOpen: number) => void,
  ) {
    super();
  }
  eq(other: TaskCheckboxWidget): boolean {
    return (
      other.checked === this.checked && other.bracketOpen === this.bracketOpen
    );
  }
  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className = "cm-lp-task " + (this.checked ? "checked" : "unchecked");
    el.setAttribute("role", "checkbox");
    el.setAttribute("aria-checked", String(this.checked));
    el.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.onToggle(this.bracketOpen);
    });
    return el;
  }
  ignoreEvent(): boolean {
    return false;
  }
}
