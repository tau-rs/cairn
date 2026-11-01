import type {
  CompletionContext,
  CompletionResult,
  CompletionSource,
} from "@codemirror/autocomplete";

/** Fire rule: an open `[[` with a partial containing no `]` or `|`, anchored at
 *  the cursor. The single regex rejects the no-`[[`, already-closed, and
 *  alias-part cases. Returns the filtered+deduped stems and the offset (within
 *  `textBefore`) where the partial starts, or null when it should not fire. */
export function wikilinkCompletionState(
  textBefore: string,
  stems: string[],
): { from: number; stems: string[] } | null {
  const m = /\[\[([^\]|]*)$/.exec(textBefore);
  if (!m) return null;
  const partial = m[1];
  const from = m.index + 2; // position right after the `[[`
  const seen = new Set<string>();
  const deduped = stems.filter((s) => (seen.has(s) ? false : seen.add(s)));
  const needle = partial.toLowerCase();
  const filtered = needle
    ? deduped.filter((s) => s.toLowerCase().includes(needle))
    : deduped;
  return { from, stems: filtered };
}

/** The text to insert when a completion is applied: the stem, plus a closing
 *  `]]` unless the text right after the cursor already starts with `]]`. */
export function wikilinkInsert(stem: string, textAfter: string): string {
  return textAfter.startsWith("]]") ? stem : stem + "]]";
}

/** CodeMirror completion source that suggests note stems inside `[[ ... ]]`.
 *  `getStems` is called per request so it always sees the current note list. */
export function wikilinkCompletionSource(
  getStems: () => string[],
): CompletionSource {
  return (context: CompletionContext): CompletionResult | null => {
    const line = context.state.doc.lineAt(context.pos);
    const textBefore = line.text.slice(0, context.pos - line.from);
    const state = wikilinkCompletionState(textBefore, getStems());
    if (!state) return null;
    return {
      from: line.from + state.from,
      validFor: /^[^\]|]*$/,
      options: state.stems.map((stem) => ({
        label: stem,
        type: "text",
        apply: (view, _completion, applyFrom, applyTo) => {
          const after = view.state.sliceDoc(applyTo, applyTo + 2);
          view.dispatch({
            changes: {
              from: applyFrom,
              to: applyTo,
              insert: wikilinkInsert(stem, after),
            },
            selection: { anchor: applyFrom + stem.length + 2 },
          });
        },
      })),
    };
  };
}
