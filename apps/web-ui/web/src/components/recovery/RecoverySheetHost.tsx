import { useCairn, useActions } from "../../app/cairnStore";
import { RecoverySheet } from "./RecoverySheet";

/** Wires RecoverySheet to the store for the small-screen shells. `side` is
 *  supplied by the shell (mobile = bottom, tablet = right). */
export function RecoverySheetHost({ side }: { side: "right" | "bottom" }) {
  const actions = useActions();
  const recovery = useCairn((s) => s.recovery);
  const currentText = useCairn((s) =>
    recovery.note ? (s.openNotes[recovery.note]?.contents ?? "") : "",
  );

  return (
    <RecoverySheet
      side={side}
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
