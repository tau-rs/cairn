import { WidgetType } from "@codemirror/view";

export class HrWidget extends WidgetType {
  eq(): boolean {
    return true;
  }
  toDOM(): HTMLElement {
    const el = document.createElement("div");
    el.className = "cm-lp-hr";
    return el;
  }
  ignoreEvent(): boolean {
    return true;
  }
}
