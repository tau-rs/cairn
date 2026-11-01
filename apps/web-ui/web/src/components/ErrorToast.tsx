import type { Toast } from "../store/store";
import { IconButton } from "./ui/IconButton";

export function ErrorToast(props: {
  errors: Toast[];
  onDismiss: (id: number) => void;
}) {
  if (props.errors.length === 0) return null;
  return (
    <div className="fixed bottom-4 right-4 z-20 flex flex-col items-end gap-2">
      {props.errors.map((e) => (
        <div
          key={e.id}
          role="alert"
          className="flex items-center gap-3 rounded border border-danger bg-danger-bg px-3 py-2 text-sm text-danger shadow-lg"
        >
          <span>{e.message}</span>
          <IconButton
            label={`dismiss: ${e.message}`}
            onClick={() => props.onDismiss(e.id)}
          >
            ✕
          </IconButton>
        </div>
      ))}
    </div>
  );
}
