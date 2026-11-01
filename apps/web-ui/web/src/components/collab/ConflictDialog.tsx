import { Modal } from "../ui/Modal";
import { Button } from "../ui/Button";

/** Calm conflict choice (replaces the old force-reload confirm dialog). Never
 *  a one-click way to lose work: "See their version" only opens a read-only
 *  view. */
export function ConflictDialog({
  open,
  onOpenChange,
  onKeepMine,
  onSeeTheirs,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onKeepMine: () => void;
  onSeeTheirs: () => void;
}) {
  const close = () => onOpenChange(false);
  return (
    <Modal
      open={open}
      onClose={close}
      title="This note also changed on another device"
      description="You can keep your version, or look at theirs first. Nothing is discarded until you choose."
    >
      <div className="flex justify-end gap-2">
        <Button
          variant="ghost"
          onClick={() => {
            onSeeTheirs();
            close();
          }}
        >
          See their version
        </Button>
        <Button variant="primary" onClick={onKeepMine}>
          Keep my version
        </Button>
      </div>
    </Modal>
  );
}
