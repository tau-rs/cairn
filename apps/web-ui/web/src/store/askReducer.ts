import type { AnswerEvent } from "../contract";

/** One message in the ask conversation. */
export interface AskTurn {
  role: "user" | "assistant";
  text: string;
  /** Distinct cited note stems, in first-seen order (assistant turns). */
  citations: string[];
  /** Tool activity; `ok === null` means still running. */
  tools: { tool: string; ok: boolean | null }[];
}

export function emptyAssistantTurn(): AskTurn {
  return { role: "assistant", text: "", citations: [], tools: [] };
}

/** Apply one content event to the in-flight assistant turn. Pure. Lifecycle
 *  events (turn_completed/completed/failed) and unknown kinds are no-ops here —
 *  the slice owns lifecycle (streaming flag, error). */
export function applyAnswerEvent(turn: AskTurn, e: AnswerEvent): AskTurn {
  switch (e.type) {
    case "sources":
      return { ...turn, citations: e.paths };
    case "text_delta":
      return { ...turn, text: turn.text + e.text };
    case "tool_started":
      return { ...turn, tools: [...turn.tools, { tool: e.tool, ok: null }] };
    case "tool_completed":
      return { ...turn, tools: markDone(turn.tools, e.tool, e.ok) };
    default:
      return turn;
  }
}

/** Mark the most recent still-running tool with this name as done. */
function markDone(
  tools: AskTurn["tools"],
  tool: string,
  ok: boolean,
): AskTurn["tools"] {
  for (let i = tools.length - 1; i >= 0; i--) {
    if (tools[i].tool === tool && tools[i].ok === null) {
      const next = tools.slice();
      next[i] = { tool, ok };
      return next;
    }
  }
  return tools;
}
