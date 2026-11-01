import { Modal } from "../ui/Modal";
import { Button } from "../ui/Button";

export function RestoreConfirmDialog(props: {
  open: boolean;
  revision: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <Modal
      open={props.open}
      onClose={props.onCancel}
      title={
        props.revision
          ? `Restore version ${props.revision}?`
          : "Restore this version?"
      }
      description="Restoring replaces the note's current contents. Your change is saved as a new version, so nothing is lost."
    >
      <div className="flex justify-end gap-2">
        <Button variant="ghost" onClick={props.onCancel}>
          Cancel
        </Button>
        <Button variant="primary" onClick={props.onConfirm}>
          Restore
        </Button>
      </div>
    </Modal>
  );
}
