import { useEffect, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { useCairn, useActions } from "../../app/cairnStore";
import { noteUrl } from "../../app/routes";
import { Input } from "../ui/Input";
import { SearchResults } from "../SearchResults";

/** Debounce for search-as-you-type; the store already drops stale responses. */
const SEARCH_DEBOUNCE_MS = 150;

/**
 * The mobile Search tab as a real destination: an autofocused input plus inline
 * results. Owned by `mobileTab === "search"`, so the tab lights up the instant
 * it's tapped and stays put while you search — the editor overlay is for desktop.
 */
export function MobileSearchView() {
  const navigate = useNavigate();
  const actions = useActions();
  const query = useCairn((s) => s.query);
  const searchResults = useCairn((s) => s.searchResults);
  const searchSnippets = useCairn((s) => s.searchSnippets);
  const activeTag = useCairn((s) => s.activeTag);
  const loading = useCairn((s) => s.loading);

  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);

  function onChange(value: string) {
    actions.setQuery(value);
    clearTimeout(timer.current);
    if (value.trim() === "") {
      actions.closeSearch();
      return;
    }
    timer.current = setTimeout(
      () => actions.runSearch(value),
      SEARCH_DEBOUNCE_MS,
    );
  }

  function runNow() {
    clearTimeout(timer.current);
    if (query.trim() !== "") actions.runSearch(query);
  }

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <Input
        autoFocus
        type="search"
        placeholder="Search…"
        aria-label="Search notes"
        value={query}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") runNow();
        }}
      />
      <div className="min-h-0 flex-1">
        {searchResults === null && !loading.search ? (
          <p className="px-1 pt-2 text-sm text-faint">
            Type to search your notes.
          </p>
        ) : (
          <SearchResults
            variant="inline"
            results={searchResults}
            loading={loading.search}
            snippets={searchSnippets ?? undefined}
            title={activeTag ? `Tagged · ${activeTag}` : undefined}
            onOpen={(p) => {
              // Done searching: drop the results (clears the highlight + overlay)
              // and hand the route over to the editor view.
              actions.closeSearch();
              actions.setUi({ mobileTab: "editor" });
              navigate(noteUrl(p));
            }}
            onClose={() => actions.closeSearch()}
          />
        )}
      </div>
    </div>
  );
}
