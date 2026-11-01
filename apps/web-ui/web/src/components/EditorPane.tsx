import { useEffect } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import { useCairn, useActions } from "../app/cairnStore";
import { noteUrl, isGraph, tagFromLocation } from "../app/routes";
import { GraphView } from "./GraphView";
import { Editor } from "./Editor";
import { TabStrip } from "./tabs/TabStrip";
import { SearchResults } from "./SearchResults";
import { ErrorBoundary } from "./ErrorBoundary";
import { Button } from "./ui/Button";
import { Spinner } from "./ui/Spinner";
import { Divider } from "./editor/Divider";
import { useBreakpoint } from "./responsive/useBreakpoint";
import { RevisionView } from "./history/RevisionView";
import { TheirVersionView } from "./collab/TheirVersionView";
import type { PaneState } from "./tabs/paneModel";

/** One editor pane: its own tab strip + editor, bound to pane `index`. */
function PaneView(props: {
  pane: PaneState;
  index: number;
  focused: boolean;
  split: boolean;
}) {
  const navigate = useNavigate();
  const actions = useActions();
  const notePaths = useCairn((s) => s.notePaths);
  const openNotes = useCairn((s) => s.openNotes);
  const editorMode = useCairn((s) => s.settings.editorMode);
  const loadRemoteImages = useCairn((s) => s.settings.loadRemoteImages);
  const loading = useCairn((s) => s.loading);

  const { pane, index, focused, split } = props;
  const activePath = pane.activePath;
  const buffer = activePath ? (openNotes[activePath]?.contents ?? "") : "";

  const tabViews = pane.tabs.map((t) => ({
    path: t.path,
    preview: t.preview,
    dirty: openNotes[t.path]?.dirty ?? false,
  }));

  return (
    <div
      className={
        "flex h-full min-w-0 flex-col " +
        (split && focused ? "ring-1 ring-inset ring-accent/60" : "")
      }
      onMouseDownCapture={() => actions.focusPane(index)}
    >
      <TabStrip
        tabs={tabViews}
        activePath={activePath}
        onSelect={(p) => actions.selectTab(p, index)}
        onPin={(p) => actions.pinTab(p, index)}
        onClose={(p) => actions.closeTab(p, index)}
        onSplit={split ? undefined : actions.splitPane}
        onClosePane={split ? () => actions.closePane(index) : undefined}
      />
      <div className="relative min-h-0 flex-1">
        {loading.note && focused && (
          <div className="absolute inset-0 z-10 flex items-center justify-center bg-bg/50">
            <Spinner label="Loading note" />
          </div>
        )}
        <Editor
          path={activePath}
          value={buffer}
          mode={editorMode}
          notePaths={notePaths}
          assetUrl={actions.assetUrl}
          loadRemoteImages={loadRemoteImages}
          onChange={actions.editBuffer}
          onOpenNote={(p) => navigate(noteUrl(p))}
          onToggleMode={() =>
            actions.setSettings({
              editorMode:
                editorMode === "livepreview" ? "source" : "livepreview",
            })
          }
          onShowHistory={focused ? actions.showHistory : undefined}
        />
      </div>
    </div>
  );
}

export function EditorPane() {
  const navigate = useNavigate();
  const location = useLocation();
  const actions = useActions();
  const activePath = useCairn((s) => s.activePath);
  const searchResults = useCairn((s) => s.searchResults);
  const searchSnippets = useCairn((s) => s.searchSnippets);
  const activeTag = useCairn((s) => s.activeTag);
  const graph = useCairn((s) => s.graph);
  const noteTags = useCairn((s) => s.noteTags);
  const suggestions = useCairn((s) => s.suggestions);
  const panes = useCairn((s) => s.panes);
  const activePane = useCairn((s) => s.activePane);
  const splitRatio = useCairn((s) => s.splitRatio);
  const loading = useCairn((s) => s.loading);
  const viewingRevision = useCairn((s) => s.viewingRevision);
  const openNotes = useCairn((s) => s.openNotes);
  const theirs = useCairn((s) => s.collab.theirs);
  const view = isGraph(location) ? "graph" : "editor";
  const bp = useBreakpoint();
  const split = panes.length > 1 && bp !== "mobile";

  useEffect(() => {
    if (viewingRevision && viewingRevision.path !== activePath) {
      actions.exitRevisionView();
    }
  }, [activePath, viewingRevision, actions]);

  return (
    <ErrorBoundary
      fallback={(reset) => (
        <div
          role="alert"
          className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center text-text"
        >
          <p className="text-sm font-medium">This view crashed.</p>
          <p className="max-w-sm text-xs text-muted">
            The rest of the app is still usable. Retry to reload just this pane.
          </p>
          <Button variant="primary" onClick={reset}>
            Retry
          </Button>
        </div>
      )}
    >
      <div className="relative h-full">
        {theirs && theirs.note === activePath ? (
          <TheirVersionView
            path={theirs.note}
            mine={openNotes[theirs.note]?.contents ?? ""}
            theirs={theirs.contents}
            onBack={() => actions.collabExitTheirs()}
            onUseTheirs={() => actions.collabReloadNow()}
          />
        ) : viewingRevision && viewingRevision.path === activePath ? (
          <RevisionView
            revision={viewingRevision.revision}
            contents={viewingRevision.contents}
            current={openNotes[viewingRevision.path]?.contents ?? ""}
            onBack={() => actions.exitRevisionView()}
            onRestore={() =>
              void actions.restoreRevision(viewingRevision.revision)
            }
          />
        ) : (
          <>
            <SearchResults
              results={searchResults}
              loading={loading.search}
              snippets={searchSnippets ?? undefined}
              title={activeTag ? `Tagged · ${activeTag}` : undefined}
              onOpen={(p) => navigate(noteUrl(p))}
              onClose={() => {
                if (tagFromLocation(location) !== null) {
                  navigate(activePath ? noteUrl(activePath) : "/");
                } else {
                  actions.closeSearch();
                }
              }}
            />
            {view === "graph" ? (
              <GraphView
                nodes={graph?.nodes ?? []}
                edges={graph?.edges ?? []}
                tagsByNote={noteTags}
                activePath={activePath}
                loading={loading.graph}
                onOpenNote={(p) => navigate(noteUrl(p))}
                suggestions={suggestions}
                onLoadSuggestions={actions.loadSuggestions}
              />
            ) : (
              <div className="flex h-full">
                <div
                  className="flex min-w-0 flex-col"
                  style={{ flexGrow: split ? splitRatio : 1, flexBasis: 0 }}
                >
                  <PaneView
                    pane={panes[0]}
                    index={0}
                    focused={activePane === 0}
                    split={split}
                  />
                </div>
                {split && (
                  <Divider ratio={splitRatio} onRatio={actions.setSplitRatio} />
                )}
                {split && (
                  <div
                    className="flex min-w-0 flex-col"
                    style={{ flexGrow: 1 - splitRatio, flexBasis: 0 }}
                  >
                    <PaneView
                      pane={panes[1]}
                      index={1}
                      focused={activePane === 1}
                      split={split}
                    />
                  </div>
                )}
              </div>
            )}
          </>
        )}
      </div>
    </ErrorBoundary>
  );
}
