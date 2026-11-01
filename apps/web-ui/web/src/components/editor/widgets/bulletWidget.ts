import { WidgetType } from "@codemirror/view";

/** Renders a styled list bullet in place of a `-`/`*`/`+` marker. */
export class BulletWidget extends WidgetType {
  eq(): boolean {
    return true;
  }
  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className = "cm-lp-bullet";
    el.textContent = "•";
    return el;
  }
  ignoreEvent(): boolean {
    return true;
  }
}
