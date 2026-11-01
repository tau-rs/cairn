import { WidgetType } from "@codemirror/view";

export class WikilinkWidget extends WidgetType {
  constructor(
    readonly label: string,
    readonly target: string | null,
    readonly onOpenNote: (path: string) => void,
  ) {
    super();
  }
  eq(other: WikilinkWidget): boolean {
    return other.label === this.label && other.target === this.target;
  }
  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className =
      "cm-lp-wikilink " + (this.target ? "resolved" : "unresolved");
    el.textContent = this.label;
    if (this.target) {
      el.style.cursor = "pointer";
      el.addEventListener("mousedown", (e) => {
        e.preventDefault();
        this.onOpenNote(this.target!);
      });
    }
    return el;
  }
  ignoreEvent(): boolean {
    return false;
  }
}
