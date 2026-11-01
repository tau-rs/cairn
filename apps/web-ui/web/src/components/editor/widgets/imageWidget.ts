import { WidgetType } from "@codemirror/view";
import type { ResolvedImage } from "../imageResolver";

export class ImageWidget extends WidgetType {
  constructor(
    readonly image: ResolvedImage,
    readonly alt: string,
    readonly block: boolean,
    readonly from: number,
    readonly onEdit: (from: number) => void,
    readonly onLoadImage: (src: string) => void,
  ) {
    super();
  }
  eq(other: ImageWidget): boolean {
    return (
      sameImage(other.image, this.image) &&
      other.alt === this.alt &&
      other.block === this.block &&
      other.from === this.from
    );
  }
  toDOM(): HTMLElement {
    switch (this.image.kind) {
      case "ready":
        return this.imageEl(this.image.url);
      case "blocked":
        return this.placeholder(this.image.src);
      case "invalid":
        return this.placeholder(null);
    }
  }
  private imageEl(url: string): HTMLImageElement {
    const img = document.createElement("img");
    img.className = this.block ? "cm-lp-img block" : "cm-lp-img";
    img.src = url;
    img.alt = this.alt;
    // The asset failed to load (missing file, asset protocol disabled/unscoped,
    // …). Mark the node so CSS renders a clear "unavailable" placeholder; the
    // browser already paints the `alt` for a broken <img>. We must NOT swap the
    // widget's DOM here (remove the <img>, insert a span): CodeMirror's
    // MutationObserver reconciles childList changes in cm-content as text edits
    // and would delete the markdown. A class/attribute change on the existing
    // node is safe.
    img.addEventListener("error", () => {
      img.classList.add("cm-lp-img-error");
      img.dataset.cairnUnavailable = "true";
    });
    img.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.onEdit(this.from);
    });
    return img;
  }
  /** Placeholder for an image that isn't being loaded. When `src` is given the
   *  image is external/`data:` and merely opt-in-gated, so a "Load" button is
   *  offered — clicking it reports the src via `onLoadImage`, and the editor
   *  re-renders the widget as a real <img> through a decoration rebuild. The
   *  widget must never swap its own DOM: CodeMirror's MutationObserver would
   *  reconcile the foreign mutation as a text edit and delete the markdown.
   *  When `src` is null the path was refused (escapes the vault); no load
   *  affordance is offered. */
  private placeholder(src: string | null): HTMLElement {
    const box = document.createElement("span");
    box.className = this.block
      ? "cm-lp-img-blocked block"
      : "cm-lp-img-blocked";
    const label = document.createElement("span");
    label.className = "cm-lp-img-blocked-label";
    label.textContent =
      src === null ? "Image path not allowed" : "External image blocked";
    box.append(label);
    if (src !== null) {
      const load = document.createElement("button");
      load.type = "button";
      load.className = "cm-lp-img-blocked-load";
      load.textContent = "Load";
      load.addEventListener("mousedown", (e) => {
        e.preventDefault();
        e.stopPropagation();
        this.onLoadImage(src);
      });
      box.append(load);
    }
    // Clicking the placeholder body (not the button) reveals raw markdown,
    // matching a rendered image's edit affordance.
    box.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.onEdit(this.from);
    });
    return box;
  }
  ignoreEvent(): boolean {
    return false;
  }
}

function sameImage(a: ResolvedImage, b: ResolvedImage): boolean {
  if (a.kind === "ready" && b.kind === "ready") return a.url === b.url;
  if (a.kind === "blocked" && b.kind === "blocked") return a.src === b.src;
  if (a.kind === "invalid" && b.kind === "invalid") return a.src === b.src;
  return false;
}
