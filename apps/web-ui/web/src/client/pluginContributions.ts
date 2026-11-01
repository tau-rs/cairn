// Untrusted-data sanitizer for Tier-2 plugin UI `contributions`.
//
// SEPARATE from the thin tag-checking `contractGuards.ts`: that module only
// verifies a backend union carries a known discriminant and THROWS on drift.
// This module is hardened against *adversarial plugin data* — its posture is
// drop-unknown + clamp, and it NEVER throws. Every malformed/oversized field
// is dropped or clamped, and a reason is recorded in the optional report so
// callers can surface what was rejected. Output objects are valid against the
// vendored `PluginContribution` / `PluginWidget` shapes (every `T | null` key
// is present, `null` when absent).

import {
  PLUGIN_ICON_VALUES,
  type PluginContribution,
  type PluginIcon,
  type PluginListItem,
  type PluginSlot,
  type PluginSummary,
  type PluginWidget,
} from "../contract";
import type { JsonValue } from "../contract/serde_json/JsonValue";
import {
  MAX_ENTRY,
  MAX_IFRAME_HEIGHT,
  MIN_IFRAME_HEIGHT,
  DEFAULT_IFRAME_HEIGHT,
  isCapability,
  type PluginCapability,
} from "./pluginTier3";

/** Local allow-lists. Independent of the contract consts on purpose (this is
 *  trust-boundary code), but a lockstep test asserts they stay supersets of
 *  the contract `PLUGIN_*_VALUES`, so contract drift can't silently widen us. */
export const PLUGIN_SLOTS = [
  "sidebar.section",
  "topbar.action",
  "command",
  "panel.main",
] as const;
export const WIDGET_KINDS = ["text", "action", "list", "iframe"] as const;
const ICONS = PLUGIN_ICON_VALUES;

export const MAX_CONTRIBS_PER_PLUGIN = 64;
export const MAX_LIST_ITEMS = 200;
export const MAX_STR = 2000;
export const MAX_ARGS_BYTES = 16384;

export type SanitizeReport = {
  kept: number;
  dropped: number;
  reasons: string[];
};
export type SlotEntry = {
  plugin: string;
  c: PluginContribution;
  epoch: number;
};

const SLOT_SET: ReadonlySet<string> = new Set(PLUGIN_SLOTS);
const KIND_SET: ReadonlySet<string> = new Set(WIDGET_KINDS);
const ICON_SET: ReadonlySet<string> = new Set(ICONS);

function isRecord(x: unknown): x is Record<string, unknown> {
  return typeof x === "object" && x !== null && !Array.isArray(x);
}

function drop(report: SanitizeReport | undefined, reason: string): null {
  if (report) {
    report.dropped += 1;
    report.reasons.push(reason);
  }
  return null;
}

/** Clamp any rendered string to MAX_STR; non-strings become "". */
function clampStr(x: unknown): string {
  if (typeof x !== "string") return "";
  return x.length > MAX_STR ? x.slice(0, MAX_STR) : x;
}

/** Coerce an out-of-enum / missing icon to null. */
function icon(x: unknown): PluginIcon | null {
  return typeof x === "string" && ICON_SET.has(x) ? (x as PluginIcon) : null;
}

/** A safe relative bundle path: non-empty, ≤MAX_ENTRY, no leading slash, no
 *  backslash, no `..`/empty segment. Returns null if unsafe. */
function safeEntry(x: unknown): string | null {
  if (typeof x !== "string" || x.length === 0 || x.length > MAX_ENTRY)
    return null;
  if (x.startsWith("/") || x.includes("\\")) return null;
  if (x.split("/").some((seg) => seg === ".." || seg === "")) return null;
  return x;
}

/** Pass `args` through if it fits the byte budget; otherwise signal oversize.
 *  Returns `{ ok: true, value }` or `{ ok: false }`. Absent args → null. */
function checkArgs(
  x: unknown,
): { ok: true; value: JsonValue | null } | { ok: false } {
  if (x === undefined || x === null) return { ok: true, value: null };
  let size: number;
  try {
    size = JSON.stringify(x).length;
  } catch {
    return { ok: false };
  }
  if (size > MAX_ARGS_BYTES) return { ok: false };
  return { ok: true, value: x as JsonValue };
}

function sanitizeListItem(
  raw: unknown,
  report: SanitizeReport | undefined,
): PluginListItem | null {
  if (!isRecord(raw)) return drop(report, "list item: not an object");
  if (typeof raw.id !== "string")
    return drop(report, "list item: missing string id");
  const args = checkArgs(raw.args);
  if (!args.ok) return drop(report, `list item ${raw.id}: args too large`);
  return {
    id: clampStr(raw.id),
    label: clampStr(raw.label),
    icon: icon(raw.icon),
    command: typeof raw.command === "string" ? clampStr(raw.command) : null,
    args: args.value,
  };
}

function sanitizeWidget(
  raw: unknown,
  report: SanitizeReport | undefined,
): PluginWidget | null {
  if (!isRecord(raw)) return drop(report, "widget: not an object");
  const kind = raw.kind;
  if (typeof kind !== "string" || !KIND_SET.has(kind))
    return drop(report, `widget: unknown kind ${JSON.stringify(kind)}`);

  if (kind === "text") {
    return {
      kind: "text",
      text: clampStr(raw.text),
      muted: typeof raw.muted === "boolean" ? raw.muted : null,
    };
  }
  if (kind === "action") {
    if (typeof raw.command !== "string")
      return drop(report, "action widget: missing string command");
    const args = checkArgs(raw.args);
    if (!args.ok) return drop(report, "action widget: args too large");
    return {
      kind: "action",
      label: clampStr(raw.label),
      icon: icon(raw.icon),
      command: clampStr(raw.command),
      args: args.value,
    };
  }
  if (kind === "iframe") {
    const entry = safeEntry(raw.entry);
    if (entry === null)
      return drop(report, "iframe widget: unsafe or missing entry");
    let height: number | null = DEFAULT_IFRAME_HEIGHT;
    if (raw.height === null) height = null;
    else if (typeof raw.height === "number" && Number.isFinite(raw.height))
      height = Math.min(
        MAX_IFRAME_HEIGHT,
        Math.max(MIN_IFRAME_HEIGHT, raw.height),
      );
    return {
      kind: "iframe",
      entry,
      height,
    };
  }
  // kind === "list"
  const itemsRaw = Array.isArray(raw.items) ? raw.items : [];
  const items: PluginListItem[] = [];
  for (const it of itemsRaw.slice(0, MAX_LIST_ITEMS)) {
    const item = sanitizeListItem(it, report);
    if (item) items.push(item);
  }
  return { kind: "list", items };
}

function sanitizeOne(
  raw: unknown,
  report: SanitizeReport | undefined,
): PluginContribution | null {
  if (!isRecord(raw)) return drop(report, "contribution: not an object");
  if (typeof raw.id !== "string")
    return drop(report, "contribution: missing string id");
  const slot = raw.slot;
  if (typeof slot !== "string" || !SLOT_SET.has(slot))
    return drop(
      report,
      `contribution ${raw.id}: unknown slot ${JSON.stringify(slot)}`,
    );

  const widget = sanitizeWidget(raw.widget, report);
  if (!widget) return null; // sanitizeWidget already recorded the drop.

  if (slot === "command" && widget.kind !== "action")
    return drop(
      report,
      `contribution ${raw.id}: command slot requires an action widget`,
    );

  if (
    (widget as unknown as { kind: string }).kind === "iframe" &&
    slot !== "sidebar.section" &&
    slot !== "panel.main"
  )
    return drop(
      report,
      `contribution ${raw.id}: iframe widget only allowed in sidebar.section or panel.main`,
    );

  return {
    id: clampStr(raw.id),
    slot: slot as PluginSlot,
    widget,
    title: typeof raw.title === "string" ? clampStr(raw.title) : null,
    icon: icon(raw.icon),
    order:
      typeof raw.order === "number" && Number.isFinite(raw.order)
        ? raw.order
        : null,
  };
}

/** Validate + clamp an untrusted contributions array. Never throws. */
export function sanitizeContributions(
  raw: unknown,
  report?: SanitizeReport,
): PluginContribution[] {
  if (!Array.isArray(raw)) return [];
  const out: PluginContribution[] = [];
  for (const item of raw.slice(0, MAX_CONTRIBS_PER_PLUGIN)) {
    const c = sanitizeOne(item, report);
    if (c) {
      out.push(c);
      if (report) report.kept += 1;
    }
  }
  return out;
}

/** Validate an untrusted `capabilities` array → known values only. Never throws.
 *  The output is deduped, so it never exceeds the distinct-capability count; the
 *  scan itself is bounded by a generous constant (not the capability count) so a
 *  long run of junk/duplicate entries can't silently push out a valid trailing
 *  capability while still capping work on an adversarially huge array. */
export function sanitizeCapabilities(raw: unknown): PluginCapability[] {
  if (!Array.isArray(raw)) return [];
  const out: PluginCapability[] = [];
  for (const v of raw.slice(0, MAX_CONTRIBS_PER_PLUGIN)) {
    if (isCapability(v) && !out.includes(v)) out.push(v);
  }
  return out;
}

const ORDER_LAST = Number.POSITIVE_INFINITY;

/** Sanitize every plugin's contributions and group the survivors by slot,
 *  each sorted by ascending `order` (null last), then plugin id, then c.id. */
export function groupBySlot(
  plugins: PluginSummary[],
  epoch: number,
  report?: SanitizeReport,
): Record<string, SlotEntry[]> {
  const grouped: Record<string, SlotEntry[]> = {};
  for (const p of plugins) {
    const clean = sanitizeContributions(p.contributions, report);
    for (const c of clean) {
      (grouped[c.slot] ??= []).push({ plugin: p.id, c, epoch });
    }
  }
  for (const slot of Object.keys(grouped)) {
    grouped[slot].sort((a, b) => {
      const oa = a.c.order ?? ORDER_LAST;
      const ob = b.c.order ?? ORDER_LAST;
      if (oa !== ob) return oa - ob;
      const byPlugin = a.plugin.localeCompare(b.plugin);
      if (byPlugin !== 0) return byPlugin;
      return a.c.id.localeCompare(b.c.id);
    });
  }
  return grouped;
}
