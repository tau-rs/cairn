export interface CommandDef {
  id: string;
  label: string;
  defaultBinding: string | null;
}

export type Overrides = Record<string, string | null>; // id → chord, or null = unbound

export const COMMAND_DEFS: CommandDef[] = [
  { id: "open-palette", label: "Command palette", defaultBinding: "Mod+K" },
  { id: "new-note", label: "New note", defaultBinding: "Mod+N" },
  {
    id: "toggle-view",
    label: "Toggle Graph / Editor",
    defaultBinding: "Mod+Shift+G",
  },
  {
    id: "toggle-editor-mode",
    label: "Toggle Source / Live preview",
    defaultBinding: "Mod+E",
  },
  { id: "open-settings", label: "Open Settings", defaultBinding: "Mod+," },
  { id: "open-ask", label: "Ask…", defaultBinding: "Mod+Shift+A" },
  { id: "close-tab", label: "Close tab", defaultBinding: "Mod+W" },
  { id: "split-right", label: "Split editor right", defaultBinding: "Mod+\\" },
  { id: "close-pane", label: "Close pane", defaultBinding: "Mod+Shift+W" },
  { id: "nav-back", label: "Back", defaultBinding: "Mod+[" },
  { id: "nav-forward", label: "Forward", defaultBinding: "Mod+]" },
  {
    id: "show-history",
    label: "Show versions",
    defaultBinding: "Mod+Shift+H",
  },
  {
    id: "recover-lost-work",
    label: "Recover lost work…",
    defaultBinding: null,
  },
];

const DEFAULT_BY_ID: Record<string, string | null> = Object.fromEntries(
  COMMAND_DEFS.map((c) => [c.id, c.defaultBinding]),
);

/** Override if present (incl. null = unbound), else the default. */
export function effectiveBinding(
  id: string,
  overrides: Overrides,
): string | null {
  return id in overrides ? overrides[id] : (DEFAULT_BY_ID[id] ?? null);
}

/** Invert effective bindings → { chord: id }, skipping unbound commands. */
export function chordToId(overrides: Overrides): Record<string, string> {
  const map: Record<string, string> = {};
  for (const def of COMMAND_DEFS) {
    const chord = effectiveBinding(def.id, overrides);
    if (chord) map[chord] = def.id;
  }
  return map;
}

/** The command currently bound to `chord` (other than `exceptId`), or null. */
export function findConflict(
  overrides: Overrides,
  chord: string,
  exceptId: string,
): string | null {
  for (const def of COMMAND_DEFS) {
    if (def.id === exceptId) continue;
    if (effectiveBinding(def.id, overrides) === chord) return def.id;
  }
  return null;
}
