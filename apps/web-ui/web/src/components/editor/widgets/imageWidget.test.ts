import { describe, it, expect, vi } from "vitest";
import { ImageWidget } from "./imageWidget";

describe("ImageWidget", () => {
  it("renders an <img> for a ready image", () => {
    const w = new ImageWidget(
      { kind: "ready", url: "asset://img/x.png" },
      "alt text",
      false,
      0,
      vi.fn(),
      vi.fn(),
    );
    const el = w.toDOM();
    expect(el).toBeInstanceOf(HTMLImageElement);
    expect((el as HTMLImageElement).src).toContain("asset://img/x.png");
    expect((el as HTMLImageElement).alt).toBe("alt text");
  });

  it("renders a placeholder (no <img>, no fetch) for a blocked image", () => {
    const w = new ImageWidget(
      { kind: "blocked", src: "https://attacker.example/beacon.png" },
      "alt",
      false,
      0,
      vi.fn(),
      vi.fn(),
    );
    const el = w.toDOM();
    expect(el).not.toBeInstanceOf(HTMLImageElement);
    // Crucially: nothing in the rendered tree carries the remote src as an
    // <img src>, so opening the note fires no network request.
    expect(el.querySelector("img")).toBeNull();
    expect(el.outerHTML).not.toContain("attacker.example");
  });

  it("renders a no-load placeholder (no <img>) for an invalid image path", () => {
    const w = new ImageWidget(
      { kind: "invalid", src: "../../etc/passwd" },
      "alt",
      false,
      0,
      vi.fn(),
      vi.fn(),
    );
    const el = w.toDOM();
    expect(el).not.toBeInstanceOf(HTMLImageElement);
    expect(el.querySelector("img")).toBeNull();
    // No empty <img src=""> is emitted, and there is no way to force-load it.
    expect(el.querySelector(".cm-lp-img-blocked-load")).toBeNull();
  });

  it("reports the src via onLoadImage when the Load button is clicked, without touching the DOM", () => {
    const onLoadImage = vi.fn();
    const w = new ImageWidget(
      { kind: "blocked", src: "https://x/y.png" },
      "alt",
      false,
      0,
      vi.fn(),
      onLoadImage,
    );
    const box = w.toDOM();
    const parent = document.createElement("div");
    parent.appendChild(box);
    const load = box.querySelector(
      ".cm-lp-img-blocked-load",
    ) as HTMLButtonElement;
    expect(load).not.toBeNull();
    load.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(onLoadImage).toHaveBeenCalledWith("https://x/y.png");
    // The widget must NOT mutate its own DOM: CodeMirror's MutationObserver
    // reconciles foreign mutations inside cm-content as text edits and would
    // delete the underlying markdown. Rendering switches via decoration
    // rebuild instead.
    expect(parent.querySelector("img")).toBeNull();
    expect(parent.contains(box)).toBe(true);
  });

  it("does not call onEdit when the Load button is clicked", () => {
    const onEdit = vi.fn();
    const w = new ImageWidget(
      { kind: "blocked", src: "https://x/y.png" },
      "alt",
      false,
      3,
      onEdit,
      vi.fn(),
    );
    const box = w.toDOM();
    document.createElement("div").appendChild(box);
    const load = box.querySelector(
      ".cm-lp-img-blocked-load",
    ) as HTMLButtonElement;
    load.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(onEdit).not.toHaveBeenCalled();
  });

  it("marks a ready <img> as unavailable on load error without restructuring DOM", () => {
    const w = new ImageWidget(
      { kind: "ready", url: "asset://missing/x.png" },
      "diagram alt",
      false,
      0,
      vi.fn(),
      vi.fn(),
    );
    const el = w.toDOM() as HTMLImageElement;
    const parent = document.createElement("div");
    parent.appendChild(el);
    el.dispatchEvent(new Event("error"));
    // Same node, still an <img>, alt preserved for native fallback rendering.
    expect(parent.firstChild).toBe(el);
    expect(el.tagName).toBe("IMG");
    expect(el.alt).toBe("diagram alt");
    expect(el.classList.contains("cm-lp-img-error")).toBe(true);
  });

  it("reveals raw markdown (onEdit) when the placeholder body is clicked", () => {
    const onEdit = vi.fn();
    const w = new ImageWidget(
      { kind: "blocked", src: "https://x/y.png" },
      "alt",
      false,
      7,
      onEdit,
      vi.fn(),
    );
    const box = w.toDOM();
    box.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(onEdit).toHaveBeenCalledWith(7);
  });
});
