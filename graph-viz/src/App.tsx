import { useCallback, useEffect, useRef, useState } from "react";
import type { Revision } from "./contract/Revision";
import { getGraph, graphAt, graphDiff, vaultHistory, DaemonError } from "./client/daemonClient";
import { buildGraphData, type RFGraph, type RFNode } from "./graph/graphData";
import { buildDiffGraph, type DiffClass } from "./graph/diffStyle";
import GraphView from "./graph/GraphView";

type Mode = "live" | "scrub" | "diff";

export default function App() {
  const [mode, setMode] = useState<Mode>("live");
  const [timeline, setTimeline] = useState<Revision[]>([]); // oldest → newest (HEAD last)
  const [graph, setGraph] = useState<RFGraph>({ nodes: [], links: [] });
  const [diffByPath, setDiffByPath] = useState<Map<string, DiffClass> | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const indexRef = useRef<Map<string, RFNode>>(new Map());

  const [scrubIdx, setScrubIdx] = useState(0);
  const [fromIdx, setFromIdx] = useState(0);
  const [toIdx, setToIdx] = useState(0);

  const apply = useCallback((nodes: any[], edges: any[]) => {
    const { graph: g, index } = buildGraphData(nodes, edges, indexRef.current);
    indexRef.current = index;
    setGraph(g);
  }, []);

  const run = useCallback(async (fn: () => Promise<void>) => {
    setLoading(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(e instanceof DaemonError ? e.message : "Can't reach daemon — is cairn-daemon running and CAIRN_VAULT set?");
    } finally {
      setLoading(false);
    }
  }, []);

  // Initial: live graph + timeline.
  useEffect(() => {
    void run(async () => {
      const g = await getGraph();
      apply(g.nodes, g.edges);
      const revs = await vaultHistory();
      const tl = [...revs].reverse(); // oldest → newest
      setTimeline(tl);
      const last = Math.max(0, tl.length - 1);
      setScrubIdx(last);
      setFromIdx(0);
      setToIdx(last);
    });
  }, [run, apply]);

  const onScrubRelease = () =>
    run(async () => {
      const rev = timeline[scrubIdx];
      if (!rev) return;
      const g = await graphAt(rev.id);
      setDiffByPath(undefined);
      apply(g.nodes, g.edges);
    });

  const onDiffRelease = () =>
    run(async () => {
      const from = timeline[fromIdx];
      const to = timeline[toIdx];
      if (!from || !to) return;
      const [base, diff] = await Promise.all([graphAt(to.id), graphDiff(from.id, to.id)]);
      const dg = buildDiffGraph(base, diff);
      setDiffByPath(new Map(dg.nodes.map((n) => [n.path, n.diff])));
      apply(dg.nodes, dg.edges);
    });

  const switchMode = (m: Mode) => {
    setMode(m);
    if (m === "live") {
      setDiffByPath(undefined);
      void run(async () => {
        const g = await getGraph();
        apply(g.nodes, g.edges);
      });
    }
  };

  const label = (r?: Revision) => (r ? `${r.id.slice(0, 7)} · ${new Date(Number(r.timestamp_secs) * 1000).toISOString().slice(0, 10)} · ${r.message}` : "");
  const maxIdx = Math.max(0, timeline.length - 1);

  return (
    <div style={{ position: "fixed", inset: 0, color: "#e6e8f0", fontFamily: "sans-serif" }}>
      <div style={{ position: "absolute", zIndex: 1, top: 12, left: 12, right: 12, display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
        <button onClick={() => switchMode("live")} aria-pressed={mode === "live"}>Live</button>
        <button onClick={() => switchMode("scrub")} aria-pressed={mode === "scrub"}>Scrub</button>
        <button onClick={() => switchMode("diff")} aria-pressed={mode === "diff"}>Diff</button>
        {loading && <span>loading…</span>}
        {error && <span style={{ color: "#ef4444" }}>{error}</span>}

        {mode === "scrub" && timeline.length > 0 && (
          <div style={{ display: "flex", flexDirection: "column", flex: 1 }}>
            <input type="range" min={0} max={maxIdx} value={scrubIdx} onChange={(e) => setScrubIdx(Number(e.target.value))} onMouseUp={onScrubRelease} onTouchEnd={onScrubRelease} />
            <small>{label(timeline[scrubIdx])}</small>
          </div>
        )}

        {mode === "diff" && timeline.length > 0 && (
          <div style={{ display: "flex", flexDirection: "column", flex: 1 }}>
            <label>from <input type="range" min={0} max={maxIdx} value={fromIdx} onChange={(e) => setFromIdx(Number(e.target.value))} onMouseUp={onDiffRelease} onTouchEnd={onDiffRelease} /></label>
            <label>to <input type="range" min={0} max={maxIdx} value={toIdx} onChange={(e) => setToIdx(Number(e.target.value))} onMouseUp={onDiffRelease} onTouchEnd={onDiffRelease} /></label>
            <small>{label(timeline[fromIdx])} → {label(timeline[toIdx])}</small>
          </div>
        )}
      </div>
      <GraphView graph={graph} diffByPath={diffByPath} />
    </div>
  );
}
