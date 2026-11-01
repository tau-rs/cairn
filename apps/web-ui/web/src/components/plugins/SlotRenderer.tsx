import { useShallow } from "zustand/react/shallow";
import { useCairn } from "../../app/cairnStore";
import type { PluginSlot } from "../../contract";
import { ErrorBoundary } from "../ErrorBoundary";
import { WidgetView } from "./WidgetView";

/**
 * Faint inline fallback for a single failed widget. Deliberately NOT the
 * app-level reload card: one bad plugin widget must degrade to a quiet retry
 * affordance, never tear down the surrounding pane.
 */
function WidgetError({ onRetry }: { onRetry: () => void }) {
  return (
    <button
      type="button"
      onClick={onRetry}
      className="text-xs text-faint italic hover:text-muted"
    >
      widget unavailable — retry
    </button>
  );
}

/**
 * Renders every plugin contribution mounted at `slot`, each isolated in its own
 * error boundary so a single throwing widget falls back to {@link WidgetError}
 * instead of taking down the host.
 *
 * The boundary `key` embeds the entry `epoch`: when `loadPlugins` re-fetches and
 * bumps the monotonic epoch, the key changes, remounting the boundary and
 * clearing any error it had previously latched.
 */
export function SlotRenderer({ slot }: { slot: PluginSlot }) {
  const here = useCairn(useShallow((s) => s.pluginContributions[slot] ?? []));
  if (here.length === 0) return null;
  return (
    <>
      {here.map(({ plugin, c, epoch }) => (
        <ErrorBoundary
          key={`${plugin}:${c.id}:${epoch}`}
          fallback={(reset) => <WidgetError onRetry={reset} />}
        >
          <WidgetView plugin={plugin} widget={c.widget} />
        </ErrorBoundary>
      ))}
    </>
  );
}
