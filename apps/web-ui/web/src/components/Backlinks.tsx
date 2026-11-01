import { SectionLabel } from "./ui/SectionLabel";
import { Spinner } from "./ui/Spinner";

export function Backlinks(props: {
  paths: string[];
  loading?: boolean;
  onOpen: (path: string) => void;
}) {
  return (
    <div className="flex flex-col gap-1 text-sm">
      <span className="mb-1">
        <SectionLabel>Backlinks</SectionLabel>
      </span>
      {props.loading ? (
        <span className="flex items-center gap-2 text-faint">
          <Spinner label="Loading backlinks" /> Loading…
        </span>
      ) : props.paths.length === 0 ? (
        <span className="text-faint">No backlinks</span>
      ) : (
        props.paths.map((path) => (
          <button
            key={path}
            className="truncate rounded px-2 py-1 text-left text-muted hover:bg-surface-2 hover:text-text"
            onClick={() => props.onOpen(path)}
          >
            {path}
          </button>
        ))
      )}
    </div>
  );
}
