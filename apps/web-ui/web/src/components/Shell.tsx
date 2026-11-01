import type { ShellRegions } from "./shells/regions";
import { PanelDock } from "./plugins/PanelDock";

export function Shell(props: ShellRegions) {
  return (
    <div className="flex h-full flex-col bg-bg text-text">
      <header className="flex items-center gap-2 border-b border-border bg-surface px-3 py-2">
        {props.topBar}
      </header>
      <div className="flex min-h-0 flex-1">
        <aside className="w-56 shrink-0 overflow-auto border-r border-border bg-surface p-2">
          {props.list}
        </aside>
        <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <div className="min-h-0 flex-1 overflow-auto p-3">{props.editor}</div>
          <PanelDock />
        </main>
        <aside className="w-56 shrink-0 overflow-auto border-l border-border bg-surface p-2">
          {props.backlinks}
        </aside>
        {props.ask}
        {props.recovery}
      </div>
    </div>
  );
}
