import { IconButton } from "./ui/IconButton";
import { SectionLabel } from "./ui/SectionLabel";
import { Spinner } from "./ui/Spinner";
import { splitSnippet, type SearchSnippet } from "./searchHighlight";

export function SearchResults(props: {
  results: string[] | null;
  loading?: boolean;
  onOpen: (path: string) => void;
  onClose: () => void;
  title?: string;
  snippets?: Record<string, SearchSnippet>;
  /** "overlay" (default) floats over the editor; "inline" fills its parent. */
  variant?: "overlay" | "inline";
}) {
  // Show the panel while a fresh search is in flight (results still null), so
  // a slow transport reads as "searching" rather than a frozen, absent panel.
  if (props.results === null && !props.loading) return null;
  const results = props.results ?? [];
  const showSpinner = props.loading && results.length === 0;
  const className =
    props.variant === "inline"
      ? "flex h-full w-full flex-col rounded border border-border bg-surface p-2"
      : "absolute left-2 top-12 z-10 flex max-h-[60vh] w-72 flex-col rounded border border-border bg-surface p-2 shadow-lg";
  return (
    <div data-testid="search-results" className={className}>
      <div className="mb-1 flex items-center justify-between">
        <SectionLabel>
          {props.title ?? "Results"}
          {props.results ? ` (${results.length})` : ""}
        </SectionLabel>
        <IconButton label="close" onClick={props.onClose}>
          ✕
        </IconButton>
      </div>
      {showSpinner ? (
        <span className="flex items-center gap-2 text-sm text-faint">
          <Spinner label="Searching" /> Searching…
        </span>
      ) : results.length === 0 ? (
        <span className="text-sm text-faint">No matches</span>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto">
          {results.map((path) => {
            const snip = props.snippets?.[path];
            return (
              <button
                key={path}
                aria-label={path}
                className="block w-full rounded px-2 py-1 text-left hover:bg-surface-2"
                onClick={() => props.onOpen(path)}
              >
                <span className="block truncate text-sm text-muted">
                  {path}
                </span>
                {snip && (
                  <span className="mt-0.5 block truncate text-xs text-faint">
                    {splitSnippet(snip.snippet, snip.highlights).map(
                      (seg, i) =>
                        seg.match ? (
                          <mark key={i} className="bg-transparent text-accent">
                            {seg.text}
                          </mark>
                        ) : (
                          <span key={i}>{seg.text}</span>
                        ),
                    )}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
