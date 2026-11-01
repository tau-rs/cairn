import { useCairn, useActions } from "../../app/cairnStore";
import { RecoveryPanel } from "./RecoveryPanel";

/** Wires the docked RecoveryPanel to the store. Passed as the `recovery`
 *  shell region; the panel self-nulls when the recovery session is closed. */
export function RecoveryPanelHost() {
  const actions = useActions();
  const recovery = useCairn((s) => s.recovery);
  const currentText = useCairn((s) =>
    recovery.note ? (s.openNotes[recovery.note]?.contents ?? "") : "",
  );

  return (
    <RecoveryPanel
      open={recovery.open}
      note={recovery.note}
      status={recovery.status}
      blocks={recovery.blocks}
      error={recovery.error}
      restoring={recovery.restoring}
      currentText={currentText}
      restoreEnabled={recovery.status === "ready"}
      onCopy={(text) => void navigator.clipboard?.writeText(text)}
      onRestore={actions.restoreVersion}
      onClose={actions.closeRecovery}
    />
  );
}
