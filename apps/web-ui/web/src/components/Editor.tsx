import { useMemo, useRef, useState } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { EditorView } from "@codemirror/view";
import { EditorSelection } from "@codemirror/state";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { livePreview } from "./editor/livePreview";
import { wikilinkCompletionSource } from "./editor/wikilinkComplete";
import { toggleCheckboxChange } from "./editor/checkboxToggle";
import { makeImageResolver } from "./editor/imageResolver";
import {
  docTheme,
  docHighlightStyle,
  markdownCodeLanguages,
} from "./editor/docTheme";
import { stem } from "../client/wikilink";
import { Button } from "./ui/Button";

export function Editor(props: {
  path: string | null;
  value: string;
  mode: "livepreview" | "source";
  notePaths: string[];
  assetUrl: (relPath: string) => string;
  loadRemoteImages: boolean;
  onChange: (value: string) => void;
  onOpenNote: (path: string) => void;
  onToggleMode: () => void;
  onShowHistory?: () => void;
}) {
  const viewRef = useRef<EditorView | null>(null);

  const resolve = useMemo(() => {
    const byStem = new Map<string, string>();
    for (const p of props.notePaths) byStem.set(stem(p), p);
    return (target: string) => byStem.get(stem(target)) ?? null;
  }, [props.notePaths]);

  const onOpenNote = props.onOpenNote;
  // Per-image "Load" opt-ins for blocked external images. Session-scoped and
  // ephemeral by design (never persisted, never widens the global setting).
  // Bumping the version remakes the resolver, which rebuilds the decorations
  // so the approved image re-renders as a real <img>.
  const approvedImages = useRef<Set<string>>(new Set());
  const [approvedVersion, setApprovedVersion] = useState(0);
  const resolveImage = useMemo(() => {
    void approvedVersion; // rebuilt whenever an image is approved
    return makeImageResolver(props.assetUrl, {
      loadRemote: props.loadRemoteImages,
      isApproved: (src) => approvedImages.current.has(src),
    });
  }, [props.assetUrl, props.loadRemoteImages, approvedVersion]);
  const extensions = useMemo(() => {
    const base = markdown({
      base: markdownLanguage,
      codeLanguages: markdownCodeLanguages,
    });
    const wikilinkAutocomplete = markdownLanguage.data.of({
      autocomplete: wikilinkCompletionSource(() => props.notePaths),
    });
    const common = [
      base,
      docTheme,
      docHighlightStyle,
      EditorView.lineWrapping,
      wikilinkAutocomplete,
    ];
    const lp = livePreview({
      resolve,
      onOpenNote,
      onToggleCheckbox: (bracketOpen: number) => {
        const view = viewRef.current;
        if (!view) return;
        const change = toggleCheckboxChange(
          view.state.doc.toString(),
          bracketOpen,
        );
        view.dispatch({ changes: change });
      },
      resolveImage,
      onLoadImage: (src: string) => {
        approvedImages.current.add(src);
        setApprovedVersion((v) => v + 1);
      },
      onEditImage: (pos: number) => {
        const view = viewRef.current;
        if (!view) return;
        view.dispatch({ selection: EditorSelection.cursor(pos) });
      },
      onEnterTableEdit: (pos: number) => {
        const view = viewRef.current;
        if (!view) return;
        view.dispatch({ selection: EditorSelection.cursor(pos) });
      },
      onCommitTable: (from: number, to: number, md: string) => {
        const view = viewRef.current;
        if (!view) return;
        view.dispatch({ changes: { from, to, insert: md } });
      },
    });
    return props.mode === "livepreview" ? [...common, lp] : common;
  }, [props.mode, props.notePaths, resolve, onOpenNote, resolveImage]);

  if (!props.path) {
    return (
      <div className="text-sm text-muted">
        No note open. Pick one from the list.
      </div>
    );
  }
  return (
    <div className="flex h-full flex-col">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-sm text-muted">{props.path}</span>
        <div className="flex items-center gap-1">
          {props.onShowHistory && (
            <Button variant="ghost" onClick={props.onShowHistory}>
              History
            </Button>
          )}
          <Button variant="ghost" onClick={props.onToggleMode}>
            {props.mode === "livepreview" ? "Source" : "Live Preview"}
          </Button>
        </div>
      </div>
      <div
        className={
          "flex-1 min-h-0 " +
          (props.mode === "livepreview"
            ? "cm-doc-livepreview"
            : "cm-doc-source")
        }
      >
        <CodeMirror
          value={props.value}
          height="100%"
          // Disable @uiw/react-codemirror's default light theme so our
          // transparent docTheme shows the graphite app background.
          theme="none"
          extensions={extensions}
          basicSetup={{
            lineNumbers: false,
            foldGutter: false,
            highlightActiveLine: false,
            highlightActiveLineGutter: false,
            // Don't highlight every other occurrence of the selected text.
            highlightSelectionMatches: false,
          }}
          onChange={props.onChange}
          onCreateEditor={(view) => {
            viewRef.current = view;
          }}
        />
      </div>
    </div>
  );
}
