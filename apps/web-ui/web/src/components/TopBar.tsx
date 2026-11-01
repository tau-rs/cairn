import { useNavigate, useLocation } from "react-router-dom";
import { useCairn, useActions, cairnStore } from "../app/cairnStore";
import { isGraph, toggleViewTarget } from "../app/routes";
import { SearchBar } from "./SearchBar";
import { IconButton } from "./ui/IconButton";
import { Logo } from "./ui/Logo";
import { Button } from "./ui/Button";
import { SlotRenderer } from "./plugins/SlotRenderer";
import { PresenceCluster } from "./collab/PresenceCluster";

export function TopBar() {
  const navigate = useNavigate();
  const location = useLocation();
  const actions = useActions();
  const query = useCairn((s) => s.query);
  const liveUpdates = useCairn((s) => s.liveUpdates);
  // Narrow selectors: subscribing to the whole `collab` object re-rendered the
  // TopBar on every `theirs` fetch, which it doesn't render.
  const collabLive = useCairn((s) => s.collab.live);
  const collabPeers = useCairn((s) => s.collab.peers);
  const conflictCount = useCairn((s) => s.collab.pendingCount);
  const view = isGraph(location) ? "graph" : "editor";

  return (
    <div className="flex w-full items-center gap-3">
      <Logo />
      <span className="hidden text-sm font-semibold text-text md:inline">
        Cairn
      </span>
      {/* Below md the dedicated mobile Search tab owns search, so hide this to
          avoid two competing inputs. Tablet/desktop keep the header bar. */}
      <div className="hidden md:block">
        <SearchBar
          value={query}
          onChange={actions.setQuery}
          onSearch={actions.runSearch}
        />
      </div>
      <Button
        variant="ghost"
        className="hidden md:inline-flex"
        onClick={() =>
          navigate(toggleViewTarget(location, cairnStore.getState().activePath))
        }
      >
        {view === "graph" ? "Editor" : "Graph"}
      </Button>
      <span className="grow" />
      <SlotRenderer slot="topbar.action" />
      <PresenceCluster
        status={liveUpdates}
        live={collabLive}
        peers={collabPeers}
        conflictCount={conflictCount}
        onConflict={() => actions.setUi({ collabConflictOpen: true })}
        onReconnect={() => void cairnStore.getState().refreshAll()}
      />
      <IconButton
        label="Settings"
        onClick={() => actions.setUi({ settingsOpen: true })}
      >
        <svg
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
      </IconButton>
    </div>
  );
}
