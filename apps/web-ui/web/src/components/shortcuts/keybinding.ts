/** Canonical chord from a KeyboardEvent, e.g. "Mod+Shift+G". "Mod" = meta||ctrl.
 *  Returns null for a pure modifier press (no real key). */
export function eventToChord(e: KeyboardEvent): string | null {
  const key = e.key;
  if (key === "Meta" || key === "Control" || key === "Shift" || key === "Alt") {
    return null;
  }
  const parts: string[] = [];
  if (e.metaKey || e.ctrlKey) parts.push("Mod");
  if (e.shiftKey) parts.push("Shift");
  if (e.altKey) parts.push("Alt");
  parts.push(normalizeKey(key));
  return parts.join("+");
}

function normalizeKey(key: string): string {
  if (key === " ") return "Space";
  return key.length === 1 ? key.toUpperCase() : key; // "Enter","Tab","," stay as-is
}

/** A bindable chord must include the Mod modifier (reject bare keys / Shift-only). */
export function isValidBinding(chord: string): boolean {
  const parts = chord.split("+");
  return parts.includes("Mod") && parts[parts.length - 1] !== "Mod";
}

/** Display form: "Mod+Shift+G" → "⌘⇧G" (mac) / "Ctrl+Shift+G" (other). */
export function formatChord(chord: string, isMac: boolean): string {
  return chord
    .split("+")
    .map((p) => {
      if (p === "Mod") return isMac ? "⌘" : "Ctrl+";
      if (p === "Shift") return isMac ? "⇧" : "Shift+";
      if (p === "Alt") return isMac ? "⌥" : "Alt+";
      if (p === "Enter") return "↵";
      return p;
    })
    .join("");
}
