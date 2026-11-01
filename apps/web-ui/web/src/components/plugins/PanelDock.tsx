import { useShallow } from "zustand/react/shallow";
import { useCairn, useActions } from "../../app/cairnStore";
import { ErrorBoundary } from "../ErrorBoundary";
import { WidgetView } from "./WidgetView";

/** Faint inline fallback for a failed dock widget (mirrors SlotRenderer). */
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
 * Bottom plugin dock for the `panel.main` slot. Renders a tablist of the slot's
 * contributions; the active one's widget mounts below in its own error boundary
 * (so one throwing widget can't tear the dock down). Collapsing keeps only the
 * tab strip visible. Returns null when no plugin contributes to panel.main, so
 * it is safe to mount unconditionally in every shell.
 */
export function PanelDock() {
  const entries = useCairn(
    useShallow((s) => s.pluginContributions["panel.main"] ?? []),
  );
  const panelDock = useCairn((s) => s.ui.panelDock);
  const { setUi } = useActions();

  if (entries.length === 0) return null;

  const active =
    entries.find((e) => e.c.id === panelDock.activeId) ?? entries[0];
  const collapsed = panelDock.collapsed;

  return (
    <section
      aria-label="Plugin panel"
      className="flex flex-col border-t border-border bg-surface text-xs"
      style={collapsed ? undefined : { height: 240 }}
    >
      <div className="flex items-center border-b border-border">
        <div
          role="tablist"
          className="flex flex-1 items-stretch overflow-x-auto"
        >
          {entries.map((e) => {
            const selected = e.c.id === active.c.id;
            return (
              <button
                key={`${e.plugin}:${e.c.id}`}
                type="button"
                role="tab"
                aria-selected={selected}
                onClick={() =>
                  setUi({
                    panelDock: { activeId: e.c.id, collapsed: false },
                  })
                }
                className={`px-3 py-1.5 ${
                  selected
                    ? "border-b-2 border-accent text-text"
                    : "text-muted hover:text-text"
                }`}
              >
                {e.c.title ?? e.plugin}
              </button>
            );
          })}
        </div>
        <button
          type="button"
          aria-label={
            collapsed ? "Expand plugin panel" : "Collapse plugin panel"
          }
          onClick={() =>
            setUi({
              panelDock: { activeId: active.c.id, collapsed: !collapsed },
            })
          }
          className="px-2 py-1.5 text-muted hover:text-text"
        >
          {collapsed ? "▴" : "▾"}
        </button>
      </div>
      {!collapsed && (
        <div className="min-h-0 flex-1 overflow-auto p-2">
          <ErrorBoundary
            key={`${active.plugin}:${active.c.id}:${active.epoch}`}
            fallback={(reset) => <WidgetError onRetry={reset} />}
          >
            <WidgetView plugin={active.plugin} widget={active.c.widget} />
          </ErrorBoundary>
        </div>
      )}
    </section>
  );
}
