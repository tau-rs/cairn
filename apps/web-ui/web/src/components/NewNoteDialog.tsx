import { useEffect, useState } from "react";
import { Modal } from "./ui/Modal";
import { Button } from "./ui/Button";
import { Input } from "./ui/Input";

export function NewNoteDialog({
  open,
  onOpenChange,
  onCreate,
  initialPath = "",
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (path: string) => void;
  initialPath?: string;
}) {
  const [path, setPath] = useState(initialPath);
  // Re-seed the field each time the dialog opens (empty for the global button,
  // "<folder>/" for the per-folder +).
  useEffect(() => {
    if (open) setPath(initialPath);
  }, [open, initialPath]);
  const close = () => onOpenChange(false);
  const submit = () => {
    const p = path.trim();
    if (!p) return;
    onCreate(p);
    close();
  };
  return (
    <Modal
      open={open}
      onClose={close}
      title="New note"
      description="Path inside the cairn"
    >
      <Input
        autoFocus
        placeholder="notes/idea.md"
        value={path}
        onChange={(e) => setPath(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit();
        }}
      />
      <div className="mt-3 flex justify-end gap-2">
        <Button variant="ghost" onClick={close}>
          Cancel
        </Button>
        <Button variant="primary" disabled={!path.trim()} onClick={submit}>
          Create
        </Button>
      </div>
    </Modal>
  );
}
