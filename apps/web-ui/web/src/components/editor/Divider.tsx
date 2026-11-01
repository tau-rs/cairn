import { useRef } from "react";

/** Keyboard nudge step for the divider (fraction of total width). */
const NUDGE = 0.02;

/** Draggable vertical divider between split panes. Reports a new left-pane
 *  fraction via `onRatio`. Uses pointer capture so a drag that ends off-element
 *  (or while the pane unmounts) cannot leak listeners. Keyboard-accessible
 *  (←/→ nudge by NUDGE). */
export function Divider(props: {
  ratio: number;
  onRatio: (ratio: number) => void;
}) {
  const ref = useRef<HTMLDivElement | null>(null);
  const dragging = useRef(false);

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    dragging.current = true;
    // Optional-chained: jsdom (test env) may not implement pointer capture.
    e.currentTarget.setPointerCapture?.(e.pointerId);
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return;
    const parent = ref.current?.parentElement;
    if (!parent) return;
    const rect = parent.getBoundingClientRect();
    if (rect.width === 0) return;
    props.onRatio((e.clientX - rect.left) / rect.width);
  };

  const onPointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    dragging.current = false;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowRight") {
      e.preventDefault();
      props.onRatio(props.ratio + NUDGE);
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      props.onRatio(props.ratio - NUDGE);
    }
  };

  const pct = Math.round(props.ratio * 100);
  return (
    // WAI-ARIA "window splitter": a focusable separator that reports its
    // position and resizes via drag or arrow keys. jsx-a11y treats role
    // "separator" as non-interactive, but the splitter pattern is interactive
    // by design — hence the scoped rule disable.
    /* eslint-disable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex */
    <div
      ref={ref}
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize panes"
      aria-valuenow={pct}
      aria-valuemin={20}
      aria-valuemax={80}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onKeyDown={onKeyDown}
      className="w-1.5 shrink-0 cursor-col-resize bg-border hover:bg-accent focus:bg-accent focus:outline-none"
    />
    /* eslint-enable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex */
  );
}
