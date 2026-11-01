import { IconButton } from "./ui/IconButton";

export function NoticeToast(props: {
  message: string | null;
  onDismiss: () => void;
}) {
  if (props.message === null) return null;
  return (
    <div className="fixed bottom-16 right-4 z-20 flex items-center gap-3 rounded border border-border bg-surface-2 px-3 py-2 text-sm text-text shadow-lg">
      <span>{props.message}</span>
      <IconButton label="dismiss notice" onClick={props.onDismiss}>
        ✕
      </IconButton>
    </div>
  );
}
