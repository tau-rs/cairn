import { useEffect, useState } from "react";
import { useCairn, useActions } from "../../app/cairnStore";
import { HistoryList } from "./HistoryList";
import { RestoreConfirmDialog } from "./RestoreConfirmDialog";

export function HistoryPane() {
  const actions = useActions();
  const activePath = useCairn((s) => s.activePath);
  const history = useCairn((s) => s.history);
  const historyPath = useCairn((s) => s.historyPath);
  const loading = useCairn((s) => s.historyLoading);
  const [pending, setPending] = useState<string | null>(null);

  // (Re)load history whenever the active note changes.
  useEffect(() => {
    void actions.loadHistory();
  }, [activePath, actions]);

  // Until the reload for the current note resolves, the loaded `history` still
  // belongs to the previous note — show the spinner instead of stale revisions.
  const stale = historyPath !== activePath;

  // No note open: loadHistory() early-returns without ever setting `history`,
  // so the generic loading check in HistoryList would spin forever. Render a
  // calm dead-end-free empty state instead of mounting the loading path.
  if (activePath === null) {
    return (
      <div className="p-2 text-sm text-muted">
        Open a note to see its versions.
      </div>
    );
  }

  return (
    <>
      <HistoryList
        revisions={stale ? null : history}
        loading={loading || stale}
        onView={(rev) => void actions.viewRevision(rev)}
        onRestore={(rev) => setPending(rev)}
        onName={(id) => actions.setUi({ nameVersionFor: id })}
      />
      <RestoreConfirmDialog
        open={pending !== null}
        revision={pending}
        onCancel={() => setPending(null)}
        onConfirm={() => {
          if (pending) void actions.restoreRevision(pending);
          setPending(null);
        }}
      />
    </>
  );
}
