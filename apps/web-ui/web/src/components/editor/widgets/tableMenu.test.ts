import { describe, it, expect, afterEach } from "vitest";
import { openTableMenu, type MenuAction } from "./tableMenu";

afterEach(() => {
  document.body.innerHTML = "";
});

const actions: MenuAction[] = [
  { label: "Insert above", run: () => {} },
  { label: "Delete row", danger: true, run: () => {} },
];

describe("openTableMenu", () => {
  it("renders a popover near the anchor on a fine pointer", () => {
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);
    openTableMenu(anchor, actions, "fine");
    const menu = document.querySelector(".cm-lp-table-menu");
    expect(menu).not.toBeNull();
    expect(menu!.querySelectorAll("[role=menuitem]").length).toBe(2);
  });
  it("renders a bottom sheet on a coarse pointer", () => {
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);
    openTableMenu(anchor, actions, "coarse");
    expect(document.querySelector(".cm-lp-table-sheet")).not.toBeNull();
  });
  it("runs the action and closes on click", () => {
    let ran = false;
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);
    openTableMenu(anchor, [{ label: "Go", run: () => (ran = true) }], "fine");
    document
      .querySelector<HTMLElement>("[role=menuitem]")!
      .dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(ran).toBe(true);
    expect(document.querySelector(".cm-lp-table-menu")).toBeNull();
  });
  it("prevents default on item mousedown so the editor caret is kept", () => {
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);
    openTableMenu(anchor, actions, "fine");
    const item = document.querySelector<HTMLElement>("[role=menuitem]")!;
    const ev = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    item.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
  });
});
