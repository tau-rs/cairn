import { useMemo } from "react";
import ForceGraph2D from "react-force-graph-2d";
import { nodeRadius, labelAlpha, type RFGraph, type RFNode } from "./graphData";
import { DIFF_COLORS, type DiffClass } from "./diffStyle";

export interface GraphViewProps {
  graph: RFGraph;
  // When present, nodes are tinted by diff class (diff mode); absent = live/scrub.
  diffByPath?: Map<string, DiffClass>;
}

const DEFAULT_COLOR = DIFF_COLORS.unchanged;

export default function GraphView({ graph, diffByPath }: GraphViewProps) {
  const nodeColor = useMemo(
    () => (node: RFNode) => {
      if (diffByPath) return DIFF_COLORS[diffByPath.get(node.id) ?? "unchanged"];
      return DEFAULT_COLOR;
    },
    [diffByPath],
  );

  return (
    <ForceGraph2D
      graphData={graph}
      backgroundColor="#0f1017"
      nodeColor={nodeColor}
      nodeRelSize={1}
      nodeVal={(n: RFNode) => Math.max(1, nodeRadius(n.degree))}
      linkColor={() => "rgba(205,208,224,0.25)"}
      nodeCanvasObjectMode={() => "after"}
      nodeCanvasObject={(node: RFNode, ctx, globalScale) => {
        const alpha = labelAlpha(globalScale);
        if (alpha <= 0) return;
        ctx.globalAlpha = alpha;
        ctx.fillStyle = "#e6e8f0";
        ctx.font = `${12 / globalScale}px sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        const r = nodeRadius(node.degree);
        ctx.fillText(node.label, node.x ?? 0, (node.y ?? 0) + r + 1);
        ctx.globalAlpha = 1;
      }}
    />
  );
}
