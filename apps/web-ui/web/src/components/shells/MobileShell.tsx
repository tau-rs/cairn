import { useEffect, useRef } from "react";
import { Link2 } from "lucide-react";
import { useLocation, useNavigate } from "react-router-dom";
import { useCairn, useActions, cairnStore } from "../../app/cairnStore";
import { isGraph, notePathFromLocation, noteUrl } from "../../app/routes";
import { IconButton } from "../ui/IconButton";
import { Drawer } from "../ui/Drawer";
import { AskSheetHost } from "../ask/AskSheetHost";
import { RecoverySheetHost } from "../recovery/RecoverySheetHost";
import { BottomNav } from "./BottomNav";
import { MoreMenu } from "./MoreMenu";
import { MobileSearchView } from "./MobileSearchView";
import type { MobileTab } from "../../store/store";
import type { ShellRegions } from "./regions";

/** Mobile (<768px): one full-screen view at a time, driven by a bottom tab bar. */
export function MobileShell({ topBar, list, editor, backlinks }: ShellRegions) {
  const navigate = useNavigate();
  const location = useLocation();
  const actions = useActions();
  const mobileTab = useCairn((s) => s.ui.mobileTab);
  const backlinksOpen = useCairn((s) => s.ui.backlinksOpen);
  const searchActive = useCairn((s) => s.searchResults !== null);

  // Opening a note/graph (e.g. tapping a file in the Files tab) or a tag filter
  // landing results should leave the Files/More overlay and reveal that content.
  // This must react to *navigation* (pathname/searchActive), NOT to the tab the
  // user picked — otherwise tapping Files while a note is open re-fires the
  // effect and bounces straight back to the editor. So `mobileTab` is read via a
  // ref (off the dep list): it gates the Search tab (route-independent, never
  // auto-exited) without re-triggering the effect on every tab change.
  const pathname = location.pathname;
  const mobileTabRef = useRef(mobileTab);
  mobileTabRef.current = mobileTab;
  useEffect(() => {
    if (mobileTabRef.current === "search") return;
    const loc = { pathname };
    if (notePathFromLocation(loc) !== null || isGraph(loc) || searchActive) {
      actions.setUi({ mobileTab: "editor" });
    }
  }, [pathname, searchActive, actions]);

  // Active highlight is derived: Files/More/Search are authoritative; editor and
  // graph are read back from the route. The trailing searchActive covers a tag
  // filter whose results render as the editor overlay.
  const active: MobileTab =
    mobileTab === "files"
      ? "files"
      : mobileTab === "more"
        ? "more"
        : mobileTab === "search"
          ? "search"
          : isGraph(location)
            ? "graph"
            : searchActive
              ? "search"
              : "editor";

  function select(tab: MobileTab) {
    actions.setUi({ mobileTab: tab });
    if (tab === "graph") {
      navigate("/graph");
    } else if (tab === "editor") {
      if (isGraph(location)) {
        const path = cairnStore.getState().activePath;
        navigate(path ? noteUrl(path) : "/");
      }
    }
  }

  const main =
    mobileTab === "files" ? (
      list
    ) : mobileTab === "more" ? (
      <MoreMenu />
    ) : mobileTab === "search" ? (
      <MobileSearchView />
    ) : (
      editor
    );

  return (
    <div className="flex h-full flex-col bg-bg text-text">
      <header className="flex items-center gap-1 border-b border-border bg-surface px-2 py-2 pt-[env(safe-area-inset-top)] [&>*:first-child]:min-w-0 [&>*:first-child]:flex-1">
        {topBar}
        <IconButton
          label="Backlinks"
          onClick={() => actions.setUi({ backlinksOpen: true })}
        >
          <Link2 size={18} aria-hidden={true} />
        </IconButton>
      </header>
      <main className="min-h-0 flex-1 overflow-auto">{main}</main>
      <BottomNav active={active} onSelect={select} />
      <Drawer
        open={backlinksOpen}
        onClose={() => actions.setUi({ backlinksOpen: false })}
        side="bottom"
        label="Backlinks"
      >
        {backlinks}
      </Drawer>
      <AskSheetHost side="bottom" />
      <RecoverySheetHost side="bottom" />
    </div>
  );
}
