export interface MenuAction {
  label: string;
  danger?: boolean;
  run: () => void;
}

export type PointerType = "fine" | "coarse";

/** Detect the primary pointer; overridable for tests. */
export function pointerType(): PointerType {
  return typeof window !== "undefined" &&
    window.matchMedia?.("(pointer: coarse)").matches
    ? "coarse"
    : "fine";
}

/** Open the table action menu: a popover anchored to `anchor` on fine pointers,
 *  a bottom sheet on coarse ones. Returns a dispose function. */
export function openTableMenu(
  anchor: HTMLElement,
  actions: MenuAction[],
  pointer: PointerType = pointerType(),
): () => void {
  const isSheet = pointer === "coarse";
  const root = document.createElement("div");
  root.className = isSheet ? "cm-lp-table-sheet" : "cm-lp-table-menu";
  root.setAttribute("role", "menu");

  const dispose = () => {
    root.remove();
    document.removeEventListener("pointerdown", onOutside, true);
    document.removeEventListener("keydown", onKey, true);
  };
  const onOutside = (e: PointerEvent) => {
    if (!root.contains(e.target as Node)) dispose();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      dispose();
    }
  };

  for (const a of actions) {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "cm-lp-table-menu-item" + (a.danger ? " danger" : "");
    item.setAttribute("role", "menuitem");
    item.addEventListener("mousedown", (e) => e.preventDefault());
    item.textContent = a.label;
    item.addEventListener("click", () => {
      dispose();
      a.run();
    });
    root.appendChild(item);
  }

  document.body.appendChild(root);

  if (!isSheet) {
    const r = anchor.getBoundingClientRect();
    root.style.position = "absolute";
    root.style.left = `${r.left + window.scrollX}px`;
    root.style.top = `${r.bottom + window.scrollY + 4}px`;
  }

  // Defer listener attach so the opening click doesn't immediately close it.
  setTimeout(() => {
    document.addEventListener("pointerdown", onOutside, true);
    document.addEventListener("keydown", onKey, true);
  }, 0);

  return dispose;
}
