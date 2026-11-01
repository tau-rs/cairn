import { Button } from "./ui/Button";

export function OpenCairn(props: { onOpen: () => void }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 bg-bg text-text">
      <h1 className="text-lg font-semibold">Cairn</h1>
      <p className="text-sm text-muted">No cairn open.</p>
      <Button variant="primary" onClick={props.onOpen}>
        Open a cairn…
      </Button>
    </div>
  );
}
