import { useMemo } from "react";
import { useShallow } from "zustand/react/shallow";
import {
  cairnStore,
  cairnClient,
  useActions,
  useCairn,
} from "../../app/cairnStore";
import type { PluginWidget } from "../../contract/PluginWidget";
import { needsConsent } from "../../store/pluginGrantsSlice";
import type { PluginCapability } from "../../client/pluginTier3";
import { sanitizeCapabilities } from "../../client/pluginContributions";
import { createStoreBrokerHost } from "../../client/pluginBrokerHost";
import { IframeHost } from "./IframeHost";
import { PermissionPrompt } from "./PermissionPrompt";
import { pluginIconNode } from "./pluginIcon";

/**
 * Renders a single host-owned plugin widget descriptor. All text flows through
 * React's auto-escaping — that is the XSS barrier; plugins never inject HTML.
 * Clicks route to the store's plugin dispatcher.
 */
export function WidgetView({
  plugin,
  widget,
}: {
  plugin: string;
  widget: PluginWidget;
}) {
  const { invokePlugin } = useActions();

  if (widget.kind === "iframe") {
    return <IframeWidget plugin={plugin} widget={widget} />;
  }

  switch (widget.kind) {
    case "text":
      return (
        <span
          className={widget.muted ? "text-xs text-faint" : "text-sm text-muted"}
        >
          {widget.text}
        </span>
      );

    case "action":
      return (
        <button
          type="button"
          className="inline-flex items-center gap-1 rounded border border-border bg-surface-2 px-2 py-1 text-xs text-text hover:border-accent"
          onClick={() =>
            void invokePlugin(plugin, widget.command, widget.args ?? null)
          }
        >
          {widget.icon && pluginIconNode(widget.icon)}
          {widget.label}
        </button>
      );

    case "list":
      return (
        <ul>
          {widget.items.map((it) => {
            const cmd = it.command;
            const content = (
              <>
                {it.icon && pluginIconNode(it.icon)}
                {it.label}
              </>
            );
            return (
              <li key={it.id}>
                {cmd != null ? (
                  <button
                    type="button"
                    className="inline-flex items-center gap-1"
                    onClick={() =>
                      void invokePlugin(plugin, cmd, it.args ?? null)
                    }
                  >
                    {content}
                  </button>
                ) : (
                  <span className="inline-flex items-center gap-1">
                    {content}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      );

    default:
      return null;
  }
}

// Module-singleton broker host: lazily initialized on first IframeWidget render,
// then stable for the lifetime of the module (IframeHost requires a stable host).
let brokerHost: ReturnType<typeof createStoreBrokerHost> | null = null;
function getBrokerHost() {
  if (!brokerHost) brokerHost = createStoreBrokerHost(cairnStore, cairnClient);
  return brokerHost;
}

function IframeWidget({
  plugin,
  widget,
}: {
  plugin: string;
  widget: Extract<PluginWidget, { kind: "iframe" }>;
}) {
  const { grantPlugin } = useActions();
  const summary = useCairn(
    useShallow((s) => s.plugins.find((p) => p.id === plugin)),
  );
  const grants = useCairn((s) => s.pluginGrants);

  // Route the declared capabilities through the shared sanitizer (drop-unknown +
  // dedupe + bound) so the audited capability filter is the one actually on the
  // path, even though the vendored type is now well-formed.
  const caps: PluginCapability[] = useMemo(
    () => sanitizeCapabilities(summary?.capabilities),
    [summary],
  );
  const version = summary?.version ?? "0";
  const commandIds = useMemo(
    () => new Set((summary?.commands ?? []).map((c) => c.id)),
    [summary],
  );
  // All-or-nothing consent: the granted set IS the declared caps once consented,
  // so building it from `caps` is correct. If partial grants are ever added, this
  // must become the intersection of caps and the stored grant.
  const grantedSet = useMemo(() => new Set(caps), [caps]);

  // Stale widget reference for an unloaded plugin → render nothing rather than a
  // misleading empty-capability consent prompt for a ghost plugin.
  if (!summary) return null;

  if (needsConsent(grants, plugin, version, caps)) {
    return (
      <PermissionPrompt
        name={summary?.name ?? plugin}
        capabilities={caps}
        onAllow={() => grantPlugin(plugin, version, caps)}
        onDeny={() => {
          /* leave unmounted; reloading plugins re-triggers the prompt */
        }}
      />
    );
  }

  return (
    <IframeHost
      plugin={plugin}
      entry={widget.entry}
      height={widget.height}
      granted={grantedSet}
      pluginCommands={commandIds}
      host={getBrokerHost()}
    />
  );
}
