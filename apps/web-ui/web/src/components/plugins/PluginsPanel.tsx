import { SectionLabel } from "../ui/SectionLabel";
import type { PluginSummary } from "../../contract";
import { useActions, useCairn } from "../../app/cairnStore";

export function PluginsPanel(props: {
  plugins: PluginSummary[];
  dropped?: number;
}) {
  const grants = useCairn((s) => s.pluginGrants);
  const { revokePlugin } = useActions();
  return (
    <div className="flex flex-col gap-1 text-sm text-text">
      <span className="mb-1">
        <SectionLabel>Plugins</SectionLabel>
      </span>
      {props.plugins.length === 0 ? (
        <span className="text-xs text-faint">No plugins loaded</span>
      ) : (
        props.plugins.map((p) => {
          const grant = grants[p.id];
          return (
            <div key={p.id} className="flex flex-col gap-0.5">
              <span className="text-muted">
                {p.name} <span className="text-faint">v{p.version}</span>
              </span>
              {p.commands.map((c) => (
                <span key={c.id} className="pl-3 text-xs text-faint">
                  {c.title}
                </span>
              ))}
              {grant && grant.granted.length > 0 && (
                <div className="mt-1 flex items-center gap-2 text-xs text-muted">
                  <span>Granted: {grant.granted.join(", ")}</span>
                  <button
                    type="button"
                    onClick={() => revokePlugin(p.id)}
                    className="rounded border border-border px-2 py-0.5 hover:border-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
                  >
                    Revoke
                  </button>
                </div>
              )}
            </div>
          );
        })
      )}
      {props.dropped ? (
        <span className="text-xs text-faint">
          {props.dropped} contribution(s) not rendered — unsupported by this
          version
        </span>
      ) : null}
    </div>
  );
}
