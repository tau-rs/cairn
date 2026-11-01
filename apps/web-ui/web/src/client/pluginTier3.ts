// Host-side Tier-3 types and capability model. Independent of the generated
// contract (which gains these fields on the next engine sync) — same posture as
// pluginContributions.ts's local WIDGET_KINDS allow-list. See the plan's
// "Contract handling" note.

/** A method a sandboxed plugin may request through the broker. */
export type BrokerMethod =
  | "host.info"
  | "ui.notice"
  | "activeNote.read"
  | "activeNote.subscribe"
  | "activeNote.write"
  | "notes.read"
  | "notes.search"
  | "command.invoke";

/** A user-grantable capability declared by a plugin. `activeNote.subscribe` is
 *  folded under `activeNote.read` (subscribing is reading), so it is NOT a
 *  separate capability — see spec §6 review note. */
export const PLUGIN_CAPABILITY_VALUES = [
  "activeNote.read",
  "activeNote.write",
  "notes.read",
  "notes.search",
  "command.invoke",
] as const;

export type PluginCapability = (typeof PLUGIN_CAPABILITY_VALUES)[number];

const CAP_SET: ReadonlySet<string> = new Set(PLUGIN_CAPABILITY_VALUES);

export function isCapability(x: unknown): x is PluginCapability {
  return typeof x === "string" && CAP_SET.has(x);
}

/** method → capability gate (null = no permission required, "silent"). */
export const CAPABILITY_OF: Record<BrokerMethod, PluginCapability | null> = {
  "host.info": null,
  "ui.notice": null,
  "activeNote.read": "activeNote.read",
  "activeNote.subscribe": "activeNote.read",
  "activeNote.write": "activeNote.write",
  "notes.read": "notes.read",
  "notes.search": "notes.search",
  "command.invoke": "command.invoke",
};

export type RiskSeverity = "high" | "normal";
export type RiskRow = { label: string; severity: RiskSeverity };

// Each capability maps to a plain-language row (or null = silent). Multiple
// capabilities can share a row label; we dedupe by label (Chrome-style grouping).
const RISK_OF: Record<PluginCapability, RiskRow | null> = {
  "activeNote.write": { label: "Modify the current note", severity: "high" },
  "notes.read": { label: "Read across your whole vault", severity: "high" },
  "notes.search": { label: "Read across your whole vault", severity: "high" },
  "activeNote.read": { label: "Read the current note", severity: "normal" },
  "command.invoke": null, // silent: plugin's own commands
};

/** Collapse a declared capability set into deduped, severity-sorted risk rows. */
export function groupCapabilities(caps: PluginCapability[]): RiskRow[] {
  const byLabel = new Map<string, RiskRow>();
  for (const cap of caps) {
    const row = RISK_OF[cap];
    if (row && !byLabel.has(row.label)) byLabel.set(row.label, row);
  }
  return [...byLabel.values()].sort((a, b) =>
    a.severity === b.severity ? 0 : a.severity === "high" ? -1 : 1,
  );
}

// The iframe widget shape now comes from the vendored contract
// (`Extract<PluginWidget, { kind: "iframe" }>`); no host-side copy needed.

// Clamp limits for iframe widgets (sanitizer + broker share these).
export const MAX_ENTRY = 512; // max length of a relative bundle entry path
export const MIN_IFRAME_HEIGHT = 80;
export const MAX_IFRAME_HEIGHT = 600;
export const DEFAULT_IFRAME_HEIGHT = 240;

// Broker runtime limits.
export const BROKER_REQUEST_TIMEOUT_MS = 5000;
export const BROKER_HANDSHAKE_TIMEOUT_MS = 3000;
export const BROKER_RATE_WINDOW_MS = 1000;
export const BROKER_RATE_MAX = 50; // inbound messages per window before dropping
export const MAX_BROKER_STR = 100_000; // clamp text params (e.g. write payloads)
