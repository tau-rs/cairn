import * as Dialog from "@radix-ui/react-dialog";
import type { ReactNode } from "react";

/** A slide-in overlay panel. `right` = side drawer (tablet); `bottom` = sheet (mobile). */
export function Drawer({
  open,
  onClose,
  side,
  label,
  children,
}: {
  open: boolean;
  onClose: () => void;
  side: "right" | "bottom";
  label: string;
  children: ReactNode;
}) {
  const pos =
    side === "right"
      ? "right-0 top-0 bottom-0 w-[min(85vw,320px)] border-l"
      : "left-0 right-0 bottom-0 max-h-[70vh] rounded-t-xl border-t pb-[env(safe-area-inset-bottom)]";
  return (
    <Dialog.Root
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/50" />
        <Dialog.Content
          aria-describedby={undefined}
          className={
            "fixed z-50 overflow-y-auto border-border bg-surface p-3 text-text shadow-2xl focus:outline-none " +
            pos
          }
        >
          <Dialog.Title className="sr-only">{label}</Dialog.Title>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
