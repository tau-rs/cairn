import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import { languages } from "@codemirror/language-data";

/** Languages available for fenced-code syntax highlighting. */
export const markdownCodeLanguages = languages;

/** Graphite-token colors for markdown prose tokens and fenced-code tokens. */
const style = HighlightStyle.define([
  { tag: t.keyword, color: "#c4b5fd" },
  { tag: [t.string, t.special(t.string)], color: "#a5d6a7" },
  { tag: [t.number, t.bool, t.null], color: "#f5b76b" },
  { tag: [t.function(t.variableName), t.labelName], color: "#7dd3fc" },
  {
    tag: [t.comment, t.lineComment, t.blockComment],
    color: "#6b6c77",
    fontStyle: "italic",
  },
  { tag: [t.typeName, t.className], color: "#7dd3fc" },
  { tag: t.propertyName, color: "#cdd0e0" },
  { tag: [t.operator, t.punctuation], color: "#9a9ba6" },
]);

export const docHighlightStyle = syntaxHighlighting(style);

/** Strips the code-editor look: transparent bg, Inter prose font, token caret/
 *  selection, comfortable padding, no focus outline. The centered measure and
 *  monospace-vs-prose split is handled in CSS via the cm-doc-* container class. */
export const docTheme = EditorView.theme(
  {
    "&": { backgroundColor: "transparent", color: "#f1f1f4" },
    "&.cm-focused": { outline: "none" },
    ".cm-content": {
      fontFamily: '"Inter Variable", Inter, system-ui, sans-serif',
      fontSize: "14px",
      lineHeight: "1.7",
      padding: "8px 0 40px",
      caretColor: "#6366f1",
    },
    ".cm-scroller": { fontFamily: "inherit" },
    ".cm-cursor": { borderLeftColor: "#6366f1" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection":
      {
        backgroundColor: "#2a2a44",
      },
    ".cm-line": { padding: "0 2px" },
  },
  { dark: true },
);
