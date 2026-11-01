import type { PluginSummary } from "../../contract";
import type { JsonValue } from "../../contract/serde_json/JsonValue";
import type { PaletteCommand } from "../command-palette/CommandPalette";

const PREFIX = "plugin:";

/**
 * Flatten loaded plugins into palette commands. Legacy `p.commands` are added
 * first; `command`-slot `action` contributions are added second and WIN on id
 * collision (richer source — they carry their own label and args).
 */
export function toPaletteCommands(plugins: PluginSummary[]): PaletteCommand[] {
  const byId = new Map<string, PaletteCommand>();
  for (const p of plugins) {
    for (const c of p.commands) {
      const id = `${PREFIX}${p.id}/${c.id}`;
      byId.set(id, { id, label: `${p.name}: ${c.title}` });
    }
  }
  for (const p of plugins) {
    for (const contrib of p.contributions) {
      if (contrib.slot !== "command" || contrib.widget.kind !== "action")
        continue;
      const id = `${PREFIX}${p.id}/${contrib.widget.command}`;
      byId.set(id, { id, label: contrib.widget.label });
    }
  }
  return [...byId.values()];
}

/**
 * Map each `command`-slot `action` contribution that carries non-null args to
 * its palette id. Used at fire-time to thread args to the engine.
 */
export function pluginCommandArgs(
  plugins: PluginSummary[],
): Record<string, JsonValue> {
  const out: Record<string, JsonValue> = {};
  for (const p of plugins) {
    for (const contrib of p.contributions) {
      if (contrib.slot !== "command" || contrib.widget.kind !== "action")
        continue;
      if (contrib.widget.args == null) continue;
      out[`${PREFIX}${p.id}/${contrib.widget.command}`] = contrib.widget.args;
    }
  }
  return out;
}

/** Parse a palette command id back to {plugin, command}; null if not a plugin id.
 *  Splits on the FIRST "/" after the prefix (command ids may contain "/"). */
export function parsePluginCommandId(
  id: string,
): { plugin: string; command: string } | null {
  if (!id.startsWith(PREFIX)) return null;
  const rest = id.slice(PREFIX.length);
  const slash = rest.indexOf("/");
  if (slash === -1) return null;
  return { plugin: rest.slice(0, slash), command: rest.slice(slash + 1) };
}
