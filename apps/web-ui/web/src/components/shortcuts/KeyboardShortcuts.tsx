import { useState } from "react";
import { SectionLabel } from "../ui/SectionLabel";
import {
  COMMAND_DEFS,
  effectiveBinding,
  findConflict,
  type Overrides,
} from "./commands";
import { eventToChord, isValidBinding, formatChord } from "./keybinding";

const IS_MAC =
  typeof navigator !== "undefined" &&
  /mac/i.test(navigator.platform || navigator.userAgent || "");

export function KeyboardShortcuts(props: {
  overrides: Overrides;
  onChange: (next: Overrides) => void;
}) {
  const [capturingId, setCapturingId] = useState<string | null>(null);
  const [conflict, setConflict] = useState<{
    chord: string;
    otherId: string;
  } | null>(null);

  const labelOf = (id: string) =>
    COMMAND_DEFS.find((c) => c.id === id)?.label ?? id;

  const assign = (id: string, chord: string, alsoUnbind?: string) => {
    const next: Overrides = { ...props.overrides };
    if (alsoUnbind) next[alsoUnbind] = null;
    next[id] = chord;
    props.onChange(next);
    setCapturingId(null);
    setConflict(null);
  };

  const reset = (id: string) => {
    const next = { ...props.overrides };
    delete next[id];
    props.onChange(next);
  };

  const onCaptureKeyDown = (id: string, e: React.KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation(); // don't let the captured chord also fire the global dispatch
    if (e.key === "Escape") {
      setCapturingId(null);
      setConflict(null);
      return;
    }
    const chord = eventToChord(e.nativeEvent);
    if (!chord || !isValidBinding(chord)) return; // wait for a valid modifier chord
    const other = findConflict(props.overrides, chord, id);
    if (other) {
      setConflict({ chord, otherId: other });
      return;
    }
    assign(id, chord);
  };

  return (
    <div className="flex flex-col gap-1 text-sm text-text">
      <span className="mb-1">
        <SectionLabel>Keyboard shortcuts</SectionLabel>
      </span>
      {COMMAND_DEFS.map((def) => {
        const binding = effectiveBinding(def.id, props.overrides);
        const overridden = def.id in props.overrides;
        const capturing = capturingId === def.id;
        return (
          <div key={def.id}>
            <div className="flex items-center justify-between gap-3 py-0.5">
              <span className="text-muted">{def.label}</span>
              <div className="flex items-center gap-2">
                {capturing ? (
                  <input
                    autoFocus
                    readOnly
                    aria-label={`press keys for ${def.label}`}
                    className="w-32 rounded border border-accent bg-surface-2 px-2 py-0.5 text-center text-[11px] text-text outline-none"
                    value="press keys…"
                    onKeyDown={(e) => onCaptureKeyDown(def.id, e)}
                    onBlur={() => {
                      setCapturingId(null);
                      setConflict(null);
                    }}
                  />
                ) : (
                  <button
                    type="button"
                    aria-label={`rebind ${def.label}`}
                    className="min-w-[3rem] rounded border border-border bg-surface-2 px-2 py-0.5 font-mono text-[11px] text-text hover:border-accent"
                    onClick={() => {
                      setConflict(null);
                      setCapturingId(def.id);
                    }}
                  >
                    {binding ? formatChord(binding, IS_MAC) : "—"}
                  </button>
                )}
                {overridden && (
                  <button
                    type="button"
                    aria-label={`reset ${def.label}`}
                    className="text-faint hover:text-text"
                    onClick={() => reset(def.id)}
                  >
                    ↺
                  </button>
                )}
              </div>
            </div>
            {capturing && conflict && (
              <div className="pb-1 text-right text-[11px] text-danger">
                already bound to {labelOf(conflict.otherId)} ·{" "}
                <button
                  type="button"
                  className="underline hover:text-text"
                  onClick={() =>
                    assign(def.id, conflict.chord, conflict.otherId)
                  }
                >
                  Force
                </button>
              </div>
            )}
          </div>
        );
      })}
      <p className="mt-2 text-[11px] text-faint">
        Built-in: ⌃Tab / ⌃⇧Tab cycle tabs · ⌘1–9 jump to tab.
      </p>
    </div>
  );
}
