import type { ForceGraphMethods } from "react-force-graph-2d";
import type { GLink, GNode } from "./graphData";

/**
 * react-force-graph mutates node objects in place (adds x/y/vx/vy) and rewrites
 * link.source/target from id strings into node references at runtime. RFNode is
 * the post-mutation node shape the canvas painters read. Centralizing it (and
 * the casts below) here means a lib upgrade breaks in one place, not silently.
 */
export interface RFNode {
  id: string;
  label: string;
  degree: number;
  // Live (server-fed) build path only; unset on the temporal/compare build.
  tags?: string[];
  mtimeSecs?: number;
  x?: number;
  y?: number;
  fx?: number; // d3 pin (set to freeze, undefined to release)
  fy?: number;
}

/** The graphData prop shape ForceGraph2D consumes (and mutates in place). */
export interface RFGraphData {
  nodes: RFNode[];
  links: GLink[];
}

/** The imperative ForceGraph2D handle, typed to our node/link shapes. */
export type FG = ForceGraphMethods<RFNode, GLink>;

/** The subset of the d3 link force we configure. react-force-graph creates the
 *  force untyped; this interface is the single place that asserts its shape. */
export interface LinkForce {
  strength: (n: number) => unknown;
  distance: (n: number) => unknown;
}

/** Adapt a string-keyed build into the shape ForceGraph2D consumes. The lib
 *  mutates these arrays in place, so this is a typed view, not a copy. */
export function asGraphData(data: {
  nodes: GNode[];
  links: GLink[];
}): RFGraphData {
  return data as RFGraphData;
}

/** The d3 link force, typed. Returns undefined before the simulation exists. */
export function linkForce(fg: FG): LinkForce | undefined {
  return fg.d3Force("link") as LinkForce | undefined;
}
