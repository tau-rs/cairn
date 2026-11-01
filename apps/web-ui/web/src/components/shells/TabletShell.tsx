import { useCairn, useActions } from "../../app/cairnStore";
import { Drawer } from "../ui/Drawer";
import { AskSheetHost } from "../ask/AskSheetHost";
import { RecoverySheetHost } from "../recovery/RecoverySheetHost";
import { Button } from "../ui/Button";
import type { ShellRegions } from "./regions";

/** Tablet (768–1023px): tree + editor side by side; backlinks in a right drawer. */
export function TabletShell({ topBar, list, editor, backlinks }: ShellRegions) {
  const actions = useActions();
  const backlinksOpen = useCairn((s) => s.ui.backlinksOpen);
  return (
    <div className="flex h-full flex-col bg-bg text-text">
      <header className="flex items-center gap-2 border-b border-border bg-surface px-3 pt-[env(safe-area-inset-top)] [&>*:first-child]:flex-1 py-2">
        {topBar}
        <Button
          variant="ghost"
          onClick={() => actions.setUi({ backlinksOpen: true })}
        >
          Links
        </Button>
      </header>
      <div className="flex min-h-0 flex-1">
        <aside className="w-56 shrink-0 overflow-auto border-r border-border bg-surface p-2">
          {list}
        </aside>
        <main className="min-w-0 flex-1 overflow-auto p-3">{editor}</main>
      </div>
      <Drawer
        open={backlinksOpen}
        onClose={() => actions.setUi({ backlinksOpen: false })}
        side="right"
        label="Backlinks"
      >
        {backlinks}
      </Drawer>
      <AskSheetHost side="right" />
      <RecoverySheetHost side="right" />
    </div>
  );
}
