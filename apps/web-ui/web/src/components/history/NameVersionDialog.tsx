import { useState } from "react";
import { Modal } from "../ui/Modal";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";

/** "Name this version" — the only manual act in the Versions model, optional
 *  and retroactive (Google-Docs named milestones). */
export function NameVersionDialog({
  open,
  onOpenChange,
  onName,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onName: (name: string) => void;
}) {
  const [name, setName] = useState("");
  const close = () => {
    setName("");
    onOpenChange(false);
  };
  const submit = () => {
    const n = name.trim();
    if (!n) return;
    onName(n);
    close();
  };
  return (
    <Modal
      open={open}
      onClose={close}
      title="Name this version"
      description="Named versions stand out in the Versions browser."
    >
      <Input
        autoFocus
        placeholder="e.g. Draft 1"
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit();
        }}
      />
      <div className="mt-3 flex justify-end gap-2">
        <Button variant="ghost" onClick={close}>
          Cancel
        </Button>
        <Button variant="primary" disabled={!name.trim()} onClick={submit}>
          Name version
        </Button>
      </div>
    </Modal>
  );
}
