import { useEffect, useRef } from "react";
import { cairnStore } from "../../app/cairnStore";
import { eventToChord } from "./keybinding";

/** Commands that must still fire even while focus is in an editable target —
 *  the global affordances a user reaches for mid-edit. */
const ALLOW_IN_EDITABLE = new Set(["open-palette"]);

/** True when a keydown target is somewhere typing/selection should win over a
 *  global chord: a form control, a contentEditable region, inside CodeMirror,
 *  or inside an open dialog. */
function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (target.isContentEditable) return true;
  if (target.closest(".cm-editor")) return true; // CodeMirror
  if (target.closest('[role="dialog"]')) return true; // open dialog
  return false;
}

/**
 * Global keydown dispatch: maps a chord to a command id via `chordMap` and runs
 * it through `runCommand`, plus the built-in (non-rebindable) tab navigation
 * (Ctrl+Tab / Mod+1-9). `runCommand` is held in a ref so the window listener
 * binds once per `chordMap` change, not on every render.
 */
export function useGlobalKeys(
  chordMap: Record<string, string>,
  runCommand: (id: string) => void,
) {
  const runCommandRef = useRef(runCommand);
  runCommandRef.current = runCommand;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const editable = isEditableTarget(e.target);
      const chord = eventToChord(e);
      const id = chord ? chordMap[chord] : undefined;
      if (id) {
        // In an editable target, only the allowlist (palette/commit) fires; a
        // bare Mod+E / Mod+W must not steal focus mid-edit.
        if (editable && !ALLOW_IN_EDITABLE.has(id)) return;
        e.preventDefault();
        runCommandRef.current(id);
        return;
      }
      // Built-in tab navigation (parameterized; not rebindable). Both branches
      // (Ctrl+Tab and Mod+1..9) are suppressed in an editable target so they
      // don't fire while typing in search or the editor.
      if (editable) return;
      const st = cairnStore.getState();
      if (e.ctrlKey && e.key === "Tab") {
        e.preventDefault();
        st.cycleTab(e.shiftKey ? -1 : 1);
      } else if ((e.metaKey || e.ctrlKey) && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        st.jumpToTab(Number(e.key));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [chordMap]);
}
