import { useEffect, useMemo, useState } from "react";
import type { Revision } from "../../contract";
import type { TemporalSelection } from "./temporalControls";
import { timelineBuckets, describeSelection } from "./timelineDensity";

/** Vault-history scrubber. `timeline` is newest-first; the UI renders
 *  oldest→newest (display index 0 = oldest = left). Two explicit modes:
 *  Browse (jump to a point) and Compare (diff two points). */
export function TemporalScrubber(props: {
  timeline: Revision[];
  selection: TemporalSelection;
  onSelect: (s: TemporalSelection) => void;
  counts: { notes: number; links: number } | null;
  delta: { added: number; removed: number } | null;
  /** When on, the timeline is sourced from `structural_revisions` (only
   *  graph-changing commits) rather than the full `vault_history`. */
  structural: boolean;
  onToggleStructural: (v: boolean) => void;
}) {
  const { timeline, selection, onSelect, counts, delta } = props;
  const { structural, onToggleStructural } = props;
  const n = timeline.length;
  const [mode, setMode] = useState<"browse" | "compare">(
    selection.kind === "compare" ? "compare" : "browse",
  );
  // compare endpoints as DISPLAY indices (0 = oldest); default full span,
  // or the incoming selection's range when it's already a compare.
  const cmpFromSelection = (): { from: number; to: number } =>
    selection.kind === "compare"
      ? { from: n - 1 - selection.from, to: n - 1 - selection.to }
      : { from: 0, to: Math.max(0, n - 1) };
  const [cmp, setCmp] = useState<{ from: number; to: number }>(
    cmpFromSelection(),
  );

  // Resync the local compare-range sliders whenever an external `compare`
  // selection changes, so they don't desync from the banner (which reads
  // `selection` directly). Browse/live selections don't drive `cmp`.
  useEffect(() => {
    if (selection.kind === "compare") {
      setCmp({
        from: n - 1 - selection.from,
        to: n - 1 - selection.to,
      });
    }
  }, [selection, n]);

  // "Structural only" swaps the timeline to a shorter source at runtime, so the
  // compare endpoints (display indices against the OLD length) can fall out of
  // range — a later Compare click would then emit an out-of-range selection that
  // silently degrades to Live. Clamp them back into the new range on any length
  // change.
  useEffect(() => {
    const max = Math.max(0, n - 1);
    setCmp((c) => ({ from: Math.min(c.from, max), to: Math.min(c.to, max) }));
  }, [n]);

  const buckets = useMemo(() => timelineBuckets(timeline), [timeline]);
  const maxBar = Math.max(1, ...buckets.map((b) => b.count));
  const { state, detail } = describeSelection(selection, timeline);

  const toTl = (d: number) => n - 1 - d; // display idx → newest-first idx

  const browseDisplay =
    selection.kind === "snapshot" ? n - 1 - selection.at : n - 1;

  const setLive = () => {
    setMode("browse");
    onSelect({ kind: "live" });
  };
  const onBrowse = (displayIdx: number) =>
    onSelect({ kind: "snapshot", at: toTl(displayIdx) });

  const emitCompare = (fromD: number, toD: number) => {
    const from = Math.max(toTl(fromD), toTl(toD)); // older = higher tl index
    const to = Math.min(toTl(fromD), toTl(toD));
    if (from === to) {
      // Collapsed range → a single snapshot; follow the banner into Browse mode
      // so the UI shows one slider instead of the two-slider compare pair.
      setMode("browse");
      onSelect({ kind: "snapshot", at: to });
    } else onSelect({ kind: "compare", from, to });
  };
  const enterCompare = () => {
    setMode("compare");
    emitCompare(cmp.from, cmp.to);
  };
  // Entering Browse must reconcile the selection so the banner (which reads
  // `selection`) can't keep showing a stale "Comparing…" under the single
  // Browse slider. A live/snapshot selection already agrees with Browse; only a
  // lingering compare needs collapsing — land on the newest snapshot.
  const enterBrowse = () => {
    setMode("browse");
    if (selection.kind === "compare") onBrowse(browseDisplay);
  };

  const segBtn = (active: boolean) =>
    "rounded px-2 py-0.5 text-[11px] " +
    (active ? "bg-accent text-accent-fg" : "text-muted hover:text-text");

  return (
    <div className="pointer-events-auto absolute inset-x-2 bottom-2 z-10 flex flex-col gap-1.5 rounded-md border border-border bg-surface/90 px-2 py-1.5">
      {/* banner — "where am I" */}
      <div className="flex items-center gap-2 text-[11px]">
        <span className="font-semibold text-text">{state}</span>
        <span className="text-muted">{detail}</span>
        {counts && (
          <span className="ml-auto rounded-full border border-border px-2 py-0.5 text-muted">
            {counts.notes} notes · {counts.links} links
          </span>
        )}
        {delta && (
          <span className="text-[11px] text-muted">
            <span className="text-emerald-400">+{delta.added}</span>
            {" / "}
            <span className="text-rose-400">−{delta.removed}</span>
          </span>
        )}
      </div>

      {/* controls */}
      <div className="flex items-center gap-2">
        <div className="flex overflow-hidden rounded border border-border">
          <button
            type="button"
            aria-pressed={mode === "browse"}
            className={segBtn(mode === "browse")}
            onClick={enterBrowse}
          >
            Browse
          </button>
          <button
            type="button"
            aria-pressed={mode === "compare"}
            className={segBtn(mode === "compare")}
            onClick={enterCompare}
          >
            Compare
          </button>
        </div>
        <button
          type="button"
          aria-pressed={selection.kind === "live"}
          className={segBtn(selection.kind === "live")}
          onClick={setLive}
        >
          Live
        </button>
        <button
          type="button"
          aria-pressed={structural}
          className={segBtn(structural)}
          title="Show only revisions that changed the link graph"
          onClick={() => onToggleStructural(!structural)}
        >
          Structural only
        </button>

        {/* histogram backdrop + range control(s) */}
        <div className="relative flex flex-1 flex-col justify-end">
          <div className="flex h-5 items-end gap-[2px]" aria-hidden="true">
            {buckets.map((b, i) => (
              <div
                key={i}
                className="flex-1 rounded-t-sm bg-border"
                style={{ height: `${8 + (b.count / maxBar) * 92}%` }}
              />
            ))}
          </div>
          {mode === "browse" ? (
            <input
              type="range"
              aria-label="Timeline position"
              className="w-full accent-accent"
              min={0}
              max={Math.max(0, n - 1)}
              step={1}
              value={browseDisplay}
              onChange={(e) => onBrowse(Number(e.target.value))}
            />
          ) : (
            <div className="flex gap-2">
              <input
                type="range"
                aria-label="Compare from"
                className="w-full accent-accent"
                min={0}
                max={Math.max(0, n - 1)}
                step={1}
                value={cmp.from}
                onChange={(e) => {
                  const from = Number(e.target.value);
                  setCmp((c) => ({ ...c, from }));
                  emitCompare(from, cmp.to);
                }}
              />
              <input
                type="range"
                aria-label="Compare to"
                className="w-full accent-accent"
                min={0}
                max={Math.max(0, n - 1)}
                step={1}
                value={cmp.to}
                onChange={(e) => {
                  const to = Number(e.target.value);
                  setCmp((c) => ({ ...c, to }));
                  emitCompare(cmp.from, to);
                }}
              />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
