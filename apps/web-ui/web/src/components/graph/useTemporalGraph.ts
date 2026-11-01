import { useEffect, useMemo, useRef, useState } from "react";
import { useCairn, useActions } from "../../app/cairnStore";
import {
  selectionToRequest,
  loadTemporalOpen,
  saveTemporalOpen,
  type TemporalSelection,
} from "./temporalControls";
import { debounce } from "../../util/timer";

const SNAPSHOT_DEBOUNCE_MS = 150;

/** Wires the vault-history scrubber to the store's temporal data. Loads the
 *  whole-vault timeline once on mount; snapshot/diff loads are debounced so
 *  dragging the scrubber doesn't fire a full-vault graph_at per tick. Returns
 *  the effective source (null in live mode → caller uses the live graph). */
export function useTemporalGraph() {
  const temporal = useCairn((s) => s.temporal);
  const actions = useActions();
  const [selection, setSelection] = useState<TemporalSelection>({
    kind: "live",
  });
  const [open, setOpenState] = useState(loadTemporalOpen);
  const setOpen = (o: boolean) => {
    setOpenState(o);
    saveTemporalOpen(o);
  };

  // "Structural only" swaps the timeline SOURCE (structural_revisions vs
  // vault_history); the engine pre-filters, so no client-side filtering here.
  const [structural, setStructuralState] = useState(false);
  const setStructural = (v: boolean) => {
    setStructuralState(v);
    // Timeline indices refer to positions in the current source array; swapping
    // sources changes its length, so snap back to live to avoid a stale index.
    setSelection({ kind: "live" });
    // Drop any loaded snapshot/diff now, alongside the selection reset, rather
    // than relying on the live-branch effect firing after the next render — the
    // intent (return to the live graph) is then explicit and batching-agnostic.
    actions.clearTemporal();
  };

  // Load the vault-wide timeline; reload when the source (structural) changes.
  useEffect(() => {
    void actions.loadVaultTimeline(structural);
  }, [actions, structural]);

  const request = useMemo(
    () => selectionToRequest(selection, temporal.timeline),
    [selection, temporal.timeline],
  );

  const requestRef = useRef(request);
  requestRef.current = request;
  const dispatch = useMemo(
    () =>
      debounce(() => {
        const r = requestRef.current;
        if (r.mode === "snapshot") void actions.loadSnapshot(r.revision);
        else if (r.mode === "compare") void actions.loadDiff(r.from, r.to);
      }, SNAPSHOT_DEBOUNCE_MS),
    [actions],
  );

  useEffect(() => {
    if (request.mode === "live") {
      dispatch.cancel();
      actions.clearTemporal();
    } else {
      dispatch();
    }
    return () => dispatch.cancel();
  }, [request, dispatch, actions]);

  const mode = request.mode;
  const source = mode === "live" ? null : temporal.snapshot;
  const diff = mode === "compare" ? temporal.diff : null;

  return {
    timeline: temporal.timeline,
    selection,
    setSelection,
    open,
    setOpen,
    mode,
    source,
    diff,
    structural,
    setStructural,
  };
}
