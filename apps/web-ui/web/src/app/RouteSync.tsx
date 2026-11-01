import { useEffect, useRef } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { cairnStore, useCairn } from "./cairnStore";
import { urlToStore, storeToUrl } from "./routeReconcile";

/**
 * The single place the URL and the store meet. Renders nothing.
 * Lane A reacts to URL changes and drives the store; Lane B reacts to
 * store-origin activePath changes and updates the URL. Both are guarded so they
 * converge in one step (see routeReconcile.ts).
 */
export function RouteSync(): null {
  const location = useLocation();
  const navigate = useNavigate();
  const activePath = useCairn((s) => s.activePath);
  // Gate on `ready` (not `cairnPath`): cairnPath is set early in init() while the
  // persisted-tab restore is still running, which would let the lanes race that
  // restore. `ready` flips only after restore finishes, so a /note/* deep link is
  // opened here *after* — and thus wins over — the restored active tab.
  const ready = useCairn((s) => s.ready);

  // Current location read by Lane B without making it a dependency.
  const locationRef = useRef(location);
  locationRef.current = location;
  // Previous activePath, so Lane B can tell store-origin changes from URL-led ones.
  const prevActiveRef = useRef<string | null>(activePath);

  // Lane A: URL -> store
  useEffect(() => {
    if (!ready) return;
    const st = cairnStore.getState();
    const effects = urlToStore({
      location,
      activePath: st.activePath,
      activeTag: st.activeTag,
      searchActive: st.searchResults !== null,
    });
    for (const e of effects) {
      if (e.kind === "filterByTag") void st.filterByTag(e.tag);
      else if (e.kind === "closeSearch") st.closeSearch();
      else if (e.kind === "loadGraph") void st.loadGraph();
      else if (e.kind === "openNote") void st.openNote(e.path);
    }
  }, [location, ready]);

  // Lane B: store -> URL
  useEffect(() => {
    const prev = prevActiveRef.current;
    prevActiveRef.current = activePath;
    if (!ready) return;
    const eff = storeToUrl({
      location: locationRef.current,
      activePath,
      prevActivePath: prev,
    });
    if (eff.kind === "navigate") navigate(eff.to, { replace: true });
  }, [activePath, ready, navigate]);

  return null;
}
