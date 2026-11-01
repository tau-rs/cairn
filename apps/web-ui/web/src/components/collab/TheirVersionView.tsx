import { Button } from "../ui/Button";
import { lineDiff } from "../history/lineDiff";
import { DiffTable } from "../history/DiffTable";

/** Read-only view of the incoming remote version, diffed against my buffer.
 *  Overwriting my buffer is the explicit "Use their version" action here —
 *  never one click from the conflict dialog. */
export function TheirVersionView(props: {
  path: string;
  mine: string;
  theirs: string;
  onBack: () => void;
  onUseTheirs: () => void;
}) {
  const rows = lineDiff(props.mine, props.theirs);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-2 border-b border-border px-3 py-2 text-sm">
        <span className="text-muted">
          Viewing incoming changes —{" "}
          <span className="text-text">{props.path}</span>
        </span>
        <span className="grow" />
        <Button variant="ghost" onClick={props.onBack}>
          ← Back to my version
        </Button>
        <Button variant="primary" onClick={props.onUseTheirs}>
          Use their version
        </Button>
      </div>
      <DiffTable rows={rows} />
    </div>
  );
}
