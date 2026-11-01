import {
  groupCapabilities,
  type PluginCapability,
} from "../../client/pluginTier3";
import { Button } from "../ui/Button";

/**
 * Grouped, plain-language consent gate shown before a Tier-3 iframe plugin is
 * mounted for the first time (or after it expands its capability set). All-or-
 * nothing: Allow grants the whole declared set; Don't run leaves it unmounted.
 */
export function PermissionPrompt({
  name,
  capabilities,
  onAllow,
  onDeny,
}: {
  name: string;
  capabilities: PluginCapability[];
  onAllow: () => void;
  onDeny: () => void;
}) {
  const rows = groupCapabilities(capabilities);
  return (
    <div className="rounded border border-border bg-surface-2 p-3 text-sm">
      <p className="mb-2 text-text">
        <strong>{name}</strong> wants to:
      </p>
      {rows.length > 0 ? (
        <ul className="mb-3 space-y-1">
          {rows.map((r) => (
            <li
              key={r.label}
              className={
                r.severity === "high" ? "font-medium text-text" : "text-muted"
              }
            >
              {r.severity === "high" ? "⚠️ " : "• "}
              {r.label}
            </li>
          ))}
        </ul>
      ) : (
        <p className="mb-3 text-muted">Run with no special access.</p>
      )}
      <div className="flex gap-2">
        <Button variant="primary" onClick={onAllow}>
          Allow
        </Button>
        <Button variant="ghost" onClick={onDeny}>
          Don&apos;t run
        </Button>
      </div>
    </div>
  );
}
