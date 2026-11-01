import { useCairn, useActions } from "../app/cairnStore";
import { BacklinksPane } from "./BacklinksPane";
import { HistoryPane } from "./history/HistoryPane";

export function RightAside() {
  const tab = useCairn((s) => s.rightTab);
  const actions = useActions();
  return (
    <div className="flex h-full flex-col">
      <div role="tablist" className="mb-2 flex gap-1 text-xs">
        <button
          role="tab"
          aria-selected={tab === "backlinks"}
          className={
            "rounded px-2 py-1 " +
            (tab === "backlinks" ? "bg-surface-2 text-text" : "text-muted")
          }
          onClick={() => actions.setRightTab("backlinks")}
        >
          Backlinks
        </button>
        <button
          role="tab"
          aria-selected={tab === "history"}
          className={
            "rounded px-2 py-1 " +
            (tab === "history" ? "bg-surface-2 text-text" : "text-muted")
          }
          onClick={() => {
            actions.setRightTab("history");
            void actions.loadHistory();
          }}
        >
          Versions
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {tab === "history" ? <HistoryPane /> : <BacklinksPane />}
      </div>
    </div>
  );
}
