import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import ForceGraph2D from "react-force-graph-2d";
import type { GraphNode, SuggestedEdge, SuggestionScope } from "../contract";
import {
  buildGraphData,
  buildGraphDataFromNodes,
  buildAdjacency,
  buildSuggestedLinks,
  buildSuggestedNodes,
  nodeRadius,
  labelAlpha,
} from "./graph/graphData";
import { capByDegree } from "./graph/globalCap";
import {
  type FilterSettings,
  applyFilters,
  loadFilter,
  saveFilter,
} from "./graph/graphFilter";
import {
  type RecencySettings,
  recencyRing,
  loadRecency,
  saveRecency,
} from "./graph/recency";
import {
  type RFNode,
  type FG,
  asGraphData,
  linkForce,
} from "./graph/forceGraphTypes";
import { IconButton } from "./ui/IconButton";
import { Spinner } from "./ui/Spinner";
import { GraphForcesPanel } from "./graph/GraphForcesPanel";
import {
  type ForceSettings,
  DEFAULT_FORCE_SETTINGS,
  loadForceSettings,
  saveForceSettings,
} from "./graph/forceSettings";
import { GraphGroupsPanel } from "./graph/GraphGroupsPanel";
import {
  type ColorGroup,
  loadColorGroups,
  saveColorGroups,
  matchGroupColor,
} from "./graph/colorGroups";
import {
  type LocalGraphSettings,
  DEPTH_RANGE,
  localSubgraph,
  loadLocalGraph,
  saveLocalGraph,
} from "./graph/localGraph";
import { TemporalScrubber } from "./graph/TemporalScrubber";
import { useTemporalGraph } from "./graph/useTemporalGraph";
import { buildCompareGraphData } from "./graph/graphData";
import {
  type SuggestionsSettings,
  loadSuggestionsSettings,
  saveSuggestionsSettings,
  suggestionScopeFor,
} from "./graph/suggestionsOverlay";

export function GraphView(props: {
  nodes: GraphNode[];
  edges: { from: string; to: string }[];
  tagsByNote: Record<string, string[]>; // path → tags, for color-group matching
  activePath: string | null;
  loading?: boolean;
  onOpenNote: (path: string) => void;
  suggestions: SuggestedEdge[] | null;
  onLoadSuggestions: (scope: SuggestionScope) => void;
}) {
  const [local, setLocal] = useState<LocalGraphSettings>(loadLocalGraph);
  const changeLocal = (next: LocalGraphSettings) => {
    setLocal(next);
    saveLocalGraph(next);
  };

  // Topology helpers work on note paths; project the enriched GraphNode[] once.
  const nodePaths = useMemo(
    () => props.nodes.map((n) => n.path),
    [props.nodes],
  );

  const temporal = useTemporalGraph();

  // Effective source: live uses props; snapshot uses the historical graph
  // (still honoring local mode); compare builds a diff-styled global graph.
  const srcNodes = useMemo(
    () =>
      temporal.source ? temporal.source.nodes.map((n) => n.path) : nodePaths,
    [temporal.source, nodePaths],
  );
  const srcEdges = useMemo(
    () => (temporal.source ? temporal.source.edges : props.edges),
    [temporal.source, props.edges],
  );
  const scrubberCounts = useMemo(
    () => ({ notes: srcNodes.length, links: srcEdges.length }),
    [srcNodes, srcEdges],
  );
  const scrubberDelta = useMemo(
    () =>
      temporal.diff
        ? {
            added:
              temporal.diff.nodes_added.length +
              temporal.diff.edges_added.length,
            removed:
              temporal.diff.nodes_removed.length +
              temporal.diff.edges_removed.length,
          }
        : null,
    [temporal.diff],
  );

  const compareData = useMemo(
    () =>
      temporal.mode === "compare" && temporal.source && temporal.diff
        ? buildCompareGraphData(temporal.source, temporal.diff)
        : null,
    [temporal.mode, temporal.source, temporal.diff],
  );

  // Global graph — memoized on [nodes, edges] ONLY, so opening a note in global
  // mode never restarts the simulation.
  const globalData = useMemo(
    () => buildGraphData(srcNodes, srcEdges),
    [srcNodes, srcEdges],
  );
  // Adjacency from a fresh string-keyed build (the `data.links` array gets
  // mutated by react-force-graph, so don't read neighbor ids from it).
  const globalAdj = useMemo(
    () => buildAdjacency(buildGraphData(srcNodes, srcEdges).links),
    [srcNodes, srcEdges],
  );

  // Local subgraph — computed ONLY when local mode is on with a note open;
  // depends on activePath/depth (the focused neighborhood genuinely changes).
  const useLocal = local.enabled && !!props.activePath;
  const localSub = useMemo(
    () =>
      useLocal
        ? localSubgraph(srcNodes, srcEdges, props.activePath, local.depth)
        : null,
    [useLocal, srcNodes, srcEdges, props.activePath, local.depth],
  );
  const localData = useMemo(
    () => (localSub ? buildGraphData(localSub.nodes, localSub.edges) : null),
    [localSub],
  );
  const localAdj = useMemo(
    () =>
      localSub
        ? buildAdjacency(buildGraphData(localSub.nodes, localSub.edges).links)
        : null,
    [localSub],
  );

  const fgRef = useRef<FG | undefined>(undefined);
  const containerRef = useRef<HTMLDivElement>(null);
  const hoverRef = useRef<string | null>(null);
  const fittedRef = useRef(false);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [forces, setForces] = useState<ForceSettings>(loadForceSettings);
  const [panelOpen, setPanelOpen] = useState(false);

  const changeForces = (next: ForceSettings) => {
    setForces(next);
    saveForceSettings(next);
  };

  const [groups, setGroups] = useState<ColorGroup[]>(loadColorGroups);
  const changeGroups = (next: ColorGroup[]) => {
    setGroups(next);
    saveColorGroups(next);
  };

  // Filter (min-degree / hidden tag groups / hide-ungrouped) and recency-ring
  // settings, held component-local + persisted like forceSettings/colorGroups.
  // They gate the live build path and node paint; driven by GraphGroupsPanel.
  const [filter, setFilter] = useState<FilterSettings>(loadFilter);
  const changeFilter = (next: FilterSettings) => {
    setFilter(next);
    saveFilter(next);
  };
  const [recency, setRecency] = useState<RecencySettings>(loadRecency);
  const changeRecency = (next: RecencySettings) => {
    setRecency(next);
    saveRecency(next);
  };
  const [suggestOverlay, setSuggestOverlay] = useState<SuggestionsSettings>(
    loadSuggestionsSettings,
  );
  const changeSuggestOverlay = (next: SuggestionsSettings) => {
    setSuggestOverlay(next);
    saveSuggestionsSettings(next);
  };

  // Live (pure, non-temporal) build path: consume the server-enriched
  // GraphNode[] so degree/tags/mtime survive to the canvas. Global view is
  // capped by degree (banner when truncated); local view is already bounded by
  // the BFS depth so it skips the cap. Both then run through applyFilters.
  const isLive = temporal.source == null;
  const liveGlobalCap = useMemo(
    () => capByDegree(props.nodes, props.edges),
    [props.nodes, props.edges],
  );
  const liveSource = useMemo(() => {
    if (useLocal && localSub) {
      const keep = new Set(localSub.nodes);
      return {
        nodes: props.nodes.filter((n) => keep.has(n.path)),
        edges: localSub.edges,
      };
    }
    return { nodes: liveGlobalCap.nodes, edges: liveGlobalCap.edges };
  }, [useLocal, localSub, props.nodes, liveGlobalCap]);
  const liveFiltered = useMemo(
    () => applyFilters(liveSource.nodes, liveSource.edges, filter, groups),
    [liveSource, filter, groups],
  );
  const liveData = useMemo(
    () => buildGraphDataFromNodes(liveFiltered.nodes, liveFiltered.edges),
    [liveFiltered],
  );
  // LOCAL mode only (Q2-C): inject the missing far endpoints of note-scoped
  // suggestions as "suggested-only" ghost nodes, so "what should I link this
  // note to?" is answerable even when the partner isn't in the local
  // neighborhood. Global / vault stays empty here → buildSuggestedLinks below
  // gets no injectedIds → unchanged (still drops non-visible endpoints).
  const injectedNodes = useMemo(() => {
    if (!useLocal || !suggestOverlay.enabled || !props.suggestions) return [];
    const visible = new Set(liveData.nodes.map((n) => n.id));
    return buildSuggestedNodes(props.suggestions, visible);
  }, [useLocal, suggestOverlay.enabled, props.suggestions, liveData]);
  const suggestedLinks = useMemo(() => {
    if (!suggestOverlay.enabled || !props.suggestions) return [];
    const visible = new Set(liveData.nodes.map((n) => n.id));
    const injectedIds = injectedNodes.length
      ? new Set(injectedNodes.map((n) => n.id))
      : undefined;
    return buildSuggestedLinks(
      props.suggestions,
      visible,
      liveData.links,
      injectedIds,
    );
  }, [suggestOverlay.enabled, props.suggestions, liveData, injectedNodes]);
  const liveDataWithSuggestions = useMemo(
    () =>
      suggestedLinks.length
        ? {
            nodes: injectedNodes.length
              ? [...liveData.nodes, ...injectedNodes]
              : liveData.nodes,
            links: [...liveData.links, ...suggestedLinks],
          }
        : liveData,
    [liveData, suggestedLinks, injectedNodes],
  );
  const liveAdj = useMemo(
    () =>
      buildAdjacency(
        buildGraphDataFromNodes(liveFiltered.nodes, liveFiltered.edges).links,
      ),
    [liveFiltered],
  );
  // Cap banner shows only in the live global overview, when nodes were dropped.
  const capTruncated = isLive && !useLocal && liveGlobalCap.truncated;

  // Scope follows the graph's own full/local mode (see suggestionScopeFor).
  const suggestionScope = suggestionScopeFor(
    suggestOverlay.enabled,
    useLocal,
    props.activePath,
  );
  // Stable string key so vault scope doesn't refetch on every note switch.
  const scopeKey = suggestionScope
    ? suggestionScope.type === "note"
      ? `note:${suggestionScope.path}`
      : "vault"
    : null;
  const onLoadSuggestions = props.onLoadSuggestions;
  useEffect(() => {
    if (suggestionScope) onLoadSuggestions(suggestionScope);
    // scopeKey encodes enabled + scope + path; refetch only when it changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scopeKey, onLoadSuggestions]);

  const forcedGlobal = temporal.mode === "compare"; // compare is global-only
  const data = compareData
    ? compareData
    : isLive
      ? liveDataWithSuggestions
      : forcedGlobal
        ? globalData
        : (localData ?? globalData);
  const adjacency = compareData
    ? buildAdjacency(compareData.links)
    : isLive
      ? liveAdj
      : forcedGlobal
        ? globalAdj
        : (localAdj ?? globalAdj);
  const rfData = asGraphData(data);

  // Size the canvas to the container.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const measure = () =>
      setSize({ width: el.clientWidth, height: el.clientHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // New graph data → allow one zoom-to-fit.
  useEffect(() => {
    fittedRef.current = false;
  }, [data]);

  // Apply force settings to the d3 simulation (imperative; forces created by
  // react-force-graph). Re-applies on settings/data/size change.
  useEffect(() => {
    const fg = fgRef.current;
    if (!fg) return;
    fg.d3Force("charge")?.strength(forces.repel);
    const link = linkForce(fg);
    link?.strength(forces.linkForce);
    link?.distance(forces.linkDistance);
    fg.d3Force("center")?.strength(forces.center);

    // Freeze = pin every node so the layout holds static (hover still repaints);
    // unfreeze clears the pins.
    for (const n of rfData.nodes) {
      if (forces.frozen) {
        n.fx = n.x;
        n.fy = n.y;
      } else {
        n.fx = undefined;
        n.fy = undefined;
      }
    }
    if (!forces.frozen) fg.d3ReheatSimulation();
  }, [forces, rfData, size.width, size.height]);

  const paintNode = useCallback(
    (node: RFNode, ctx: CanvasRenderingContext2D, scale: number) => {
      const h = hoverRef.current;
      let hl: Set<string> | null = null;
      if (h) {
        hl = new Set<string>([h]);
        for (const n of adjacency.get(h) ?? []) hl.add(n);
      }
      const active = node.id === props.activePath;
      const inHL = hl ? hl.has(node.id) : true;
      const r = nodeRadius(node.degree);
      // Suggested-only ghost (Q2-C local inject): not a real graph node — render
      // muted with a dashed ring and no recency halo, so it reads as "candidate
      // to link", not part of the note's real neighborhood.
      const suggested = (node as { suggested?: boolean }).suggested === true;
      const nodeState = (node as { state?: string }).state;
      const stateColor =
        nodeState === "appeared"
          ? "#22c55e"
          : nodeState === "disappeared"
            ? "#6b7280"
            : null;
      const base = suggested
        ? "#8b8fa3"
        : (stateColor ??
          (active
            ? "#6366f1"
            : (matchGroupColor(
                node.id,
                props.tagsByNote[node.id] ?? [],
                groups,
              ) ?? "#cdd0e0")));

      ctx.beginPath();
      ctx.arc(node.x ?? 0, node.y ?? 0, r, 0, 2 * Math.PI);
      // Hover focus: dim non-neighbors (keep their group hue at low alpha).
      // Disappeared ghosts and unchanged (compare-mode base) nodes render at a
      // fixed low alpha regardless of hover, so the appeared/disappeared deltas
      // pop against a dimmed base. `state` is only set in compare mode, so live
      // mode is unaffected.
      ctx.globalAlpha = suggested
        ? 0.6
        : nodeState === "disappeared"
          ? 0.4
          : nodeState === "unchanged" || nodeState === "changed"
            ? 0.5
            : hl && !inHL && !active
              ? 0.25
              : 1;
      ctx.fillStyle = base;
      ctx.fill();
      ctx.globalAlpha = 1;

      // Compare-mode "changed": a node present in both revisions whose metadata
      // (degree/tags) shifted. Keep the base fill and add an amber ring so it
      // reads as "same node, altered" — distinct from green-appeared and
      // gray-disappeared. Amber is free here: the recency ring that also uses it
      // only draws on the live path (mtimeSecs is unset on temporal builds).
      if (nodeState === "changed") {
        ctx.beginPath();
        ctx.arc(node.x ?? 0, node.y ?? 0, r + 2, 0, 2 * Math.PI);
        ctx.strokeStyle = "#f59e0b";
        ctx.lineWidth = 1.5;
        ctx.stroke();
      }

      // Dashed ghost ring marks a suggested-only node.
      if (suggested) {
        ctx.beginPath();
        ctx.arc(node.x ?? 0, node.y ?? 0, r + 2, 0, 2 * Math.PI);
        ctx.strokeStyle = "#8b8fa3";
        ctx.lineWidth = 1;
        ctx.globalAlpha = 0.8;
        ctx.setLineDash([2, 2]);
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.globalAlpha = 1;
      }

      // Recency ring: an outer halo whose brightness/width fades with the note's
      // age, drawn only in the live path (mtimeSecs is unset on temporal builds).
      if (recency.enabled && node.mtimeSecs !== undefined) {
        const ring = recencyRing(
          node.mtimeSecs,
          Date.now() / 1000,
          recency.windowDays,
        );
        if (ring) {
          ctx.beginPath();
          ctx.arc(node.x ?? 0, node.y ?? 0, r + 2.5, 0, 2 * Math.PI);
          ctx.strokeStyle = `rgba(245, 158, 11, ${ring.alpha})`;
          ctx.lineWidth = ring.width;
          ctx.stroke();
        }
      }

      let alpha = labelAlpha(scale);
      if (active || node.id === hoverRef.current) alpha = 1;
      if (alpha > 0) {
        const font = 12 / scale; // constant on-screen size
        ctx.font = `${font}px Inter, system-ui, sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        ctx.globalAlpha = alpha;
        ctx.fillStyle = active ? "#cdd0e0" : "#9a9ba6";
        ctx.fillText(node.label, node.x ?? 0, (node.y ?? 0) + r + 1);
        ctx.globalAlpha = 1;
      }
    },
    [props.activePath, adjacency, groups, props.tagsByNote, recency],
  );

  const paintPointer = useCallback(
    (node: RFNode, color: string, ctx: CanvasRenderingContext2D) => {
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(
        node.x ?? 0,
        node.y ?? 0,
        nodeRadius(node.degree),
        0,
        2 * Math.PI,
      );
      ctx.fill();
    },
    [],
  );

  // Link colors react to hover (links touching the hovered node light up);
  // diff state (compare mode) takes priority over hover styling.
  const linkColor = useCallback(
    (link: {
      source: RFNode | string;
      target: RFNode | string;
      state?: string;
      kind?: string;
    }) => {
      if (link.kind === "suggested") return "#8b8fa3aa";
      const st = link.state;
      if (st === "appeared") return "#22c55e";
      if (st === "disappeared") return "#6b728066";
      if (st === "unchanged") return "#2a2a30";
      const h = hoverRef.current;
      if (!h) return "#3a3a44";
      const sid =
        typeof link.source === "string" ? link.source : link.source.id;
      const tid =
        typeof link.target === "string" ? link.target : link.target.id;
      return sid === h || tid === h ? "#6366f1aa" : "#26262e66";
    },
    [],
  );

  // Dashed styling for disappeared (removed) links in compare mode, and for
  // suggested links in the overlay.
  const linkLineDash = useCallback(
    (link: { state?: string; kind?: string }) =>
      link.kind === "suggested"
        ? [4, 4]
        : link.state === "disappeared"
          ? [4, 3]
          : [],
    [],
  );

  // Suggested links get width from their similarity weight (0..1); real links
  // stay at the constant 1.
  const linkWidth = useCallback(
    (link: { kind?: string; weight?: number }) =>
      link.kind === "suggested" ? 0.5 + (link.weight ?? 0) * 2 : 1,
    [],
  );

  // Native hover tooltip: suggested links show their "why" text.
  // NOTE: react-force-graph injects this string via innerHTML (its only DOM
  // sink here — every other label is canvas-painted). `why` is engine-derived
  // provenance from the user's own vault (full-trust local engine), so it is
  // treated as safe. If `why` ever carries untrusted or raw note text, escape
  // it here before returning.
  const linkLabel = useCallback(
    (link: { kind?: string; why?: string | null }) =>
      link.kind === "suggested" ? (link.why ?? "") : "",
    [],
  );

  // Single stable container (the ref/ResizeObserver always track THIS div);
  // ForceGraph2D mounts once the container has a measured size.
  return (
    <div ref={containerRef} className="relative h-full w-full">
      {props.loading && (
        <div className="absolute inset-0 z-20 flex items-center justify-center bg-bg/50">
          <Spinner label="Loading graph" />
        </div>
      )}
      {capTruncated && (
        <div
          role="status"
          className="absolute left-1/2 top-2 z-10 -translate-x-1/2 rounded-md border border-border bg-surface px-2.5 py-1 text-[11px] text-muted shadow-lg"
        >
          Showing {liveData.nodes.length} most-connected of{" "}
          {liveGlobalCap.total} notes
        </div>
      )}
      <div className="absolute left-2 top-2 z-10 flex overflow-hidden rounded-md border border-border text-[11px]">
        {(["local", "global"] as const).map((m) => {
          const isLocal = m === "local";
          const selected = local.enabled === isLocal;
          const compareLocked = temporal.mode === "compare";
          return (
            <button
              key={m}
              type="button"
              aria-pressed={selected}
              disabled={compareLocked}
              title={
                compareLocked ? "Compare shows the whole graph" : undefined
              }
              className={
                "px-2.5 py-1 capitalize disabled:cursor-not-allowed disabled:opacity-50 " +
                (selected
                  ? "bg-accent text-accent-fg"
                  : "bg-surface text-muted hover:text-text")
              }
              onClick={() => changeLocal({ ...local, enabled: isLocal })}
            >
              {m}
            </button>
          );
        })}
      </div>
      <div className="absolute right-2 top-2 z-10 flex flex-col items-end gap-2">
        <IconButton
          label="Graph forces"
          className="border border-border bg-surface"
          onClick={() => setPanelOpen((o) => !o)}
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        </IconButton>
        <IconButton
          label="Graph history"
          className="border border-border bg-surface"
          title="Graph history"
          onClick={() => temporal.setOpen(!temporal.open)}
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="12" cy="12" r="9" />
            <path d="M12 7v5l3 3" />
          </svg>
        </IconButton>
        {panelOpen && (
          <>
            <div className="w-52 rounded-lg border border-border bg-surface p-3 shadow-2xl">
              <div className="mb-2 text-[10px] uppercase tracking-wide text-faint">
                Local graph
              </div>
              <div className="flex items-center justify-between gap-2 text-[11px] text-text">
                <span>Depth</span>
                <span className="text-faint">{local.depth}</span>
              </div>
              <input
                type="range"
                aria-label="Local graph depth"
                className="w-full accent-accent"
                min={DEPTH_RANGE.min}
                max={DEPTH_RANGE.max}
                step={DEPTH_RANGE.step}
                value={local.depth}
                onChange={(e) =>
                  changeLocal({ ...local, depth: Number(e.target.value) })
                }
              />
            </div>
            <GraphGroupsPanel
              groups={groups}
              onChange={changeGroups}
              filter={filter}
              onFilterChange={changeFilter}
              recency={recency}
              onRecencyChange={changeRecency}
              suggestions={suggestOverlay}
              onSuggestionsChange={changeSuggestOverlay}
            />
            <GraphForcesPanel
              settings={forces}
              onChange={changeForces}
              onReset={() => changeForces(DEFAULT_FORCE_SETTINGS)}
            />
          </>
        )}
      </div>
      {local.enabled && !props.activePath ? (
        <div className="flex h-full w-full items-center justify-center text-sm text-faint">
          Open a note to see its local graph
        </div>
      ) : (
        size.width > 0 &&
        size.height > 0 && (
          <ForceGraph2D
            ref={fgRef}
            width={size.width}
            height={size.height}
            graphData={rfData}
            backgroundColor="rgba(0,0,0,0)"
            nodeCanvasObject={paintNode}
            nodePointerAreaPaint={paintPointer}
            linkColor={linkColor}
            linkLineDash={linkLineDash}
            linkWidth={linkWidth}
            linkLabel={linkLabel}
            autoPauseRedraw={false}
            enableNodeDrag
            onNodeClick={(n: RFNode) => props.onOpenNote(n.id)}
            onNodeHover={(n: RFNode | null) => {
              hoverRef.current = n?.id ?? null;
            }}
            onEngineStop={() => {
              if (!fittedRef.current) {
                fittedRef.current = true;
                fgRef.current?.zoomToFit(400, 40);
              }
            }}
          />
        )
      )}
      {temporal.open && temporal.timeline && (
        <TemporalScrubber
          timeline={temporal.timeline}
          selection={temporal.selection}
          onSelect={temporal.setSelection}
          counts={scrubberCounts}
          delta={scrubberDelta}
          structural={temporal.structural}
          onToggleStructural={temporal.setStructural}
        />
      )}
    </div>
  );
}
