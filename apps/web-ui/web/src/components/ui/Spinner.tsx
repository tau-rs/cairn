/** Small inline spinner. `role="status"` + a label so screen readers announce
 *  it, and so consumers can render a pending state distinct from "empty". */
export function Spinner(props: { label?: string; className?: string }) {
  return (
    <span
      role="status"
      aria-label={props.label ?? "Loading"}
      className={
        "inline-block h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent " +
        (props.className ?? "")
      }
    />
  );
}
