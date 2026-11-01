import type { ComponentProps } from "react";
import { Drawer } from "../ui/Drawer";
import { RecoveryPanel } from "./RecoveryPanel";

/** Tablet/mobile recovery surface: the RecoveryPanel inside a slide-in Drawer.
 *  `side="bottom"` is the mobile sheet, `side="right"` the tablet side sheet.
 *  RecoveryPanel's own ResizeObserver picks the "unified" layout in the narrow
 *  drawer automatically — no layout prop is forced here. */
export function RecoverySheet(
  props: ComponentProps<typeof RecoveryPanel> & { side: "right" | "bottom" },
) {
  const { side, ...panelProps } = props;
  return (
    <Drawer
      open={props.open}
      onClose={props.onClose}
      side={side}
      label="Recovery"
    >
      <div data-testid="recovery-sheet" className="h-full">
        <RecoveryPanel {...panelProps} />
      </div>
    </Drawer>
  );
}
