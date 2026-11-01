import type { SuggestionScope } from "../../contract";

export interface SuggestionsSettings {
  enabled: boolean;
}
export const DEFAULT_SUGGESTIONS_SETTINGS: SuggestionsSettings = {
  enabled: false,
};

const STORAGE_KEY = "cairn.graph.suggestions";

/** Suggestion scope follows the graph's own full/local mode: local mode with a
 *  note open → that note's suggestions; otherwise the whole vault. null when the
 *  overlay is off (no query should fire). */
export function suggestionScopeFor(
  enabled: boolean,
  localEnabled: boolean,
  activePath: string | null,
): SuggestionScope | null {
  if (!enabled) return null;
  if (localEnabled && activePath) return { type: "note", path: activePath };
  return { type: "vault" };
}

export function loadSuggestionsSettings(): SuggestionsSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_SUGGESTIONS_SETTINGS;
    const p = JSON.parse(raw) as Partial<SuggestionsSettings>;
    return { enabled: !!p.enabled };
  } catch {
    return DEFAULT_SUGGESTIONS_SETTINGS;
  }
}

export function saveSuggestionsSettings(s: SuggestionsSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    // ignore (private mode / quota)
  }
}
