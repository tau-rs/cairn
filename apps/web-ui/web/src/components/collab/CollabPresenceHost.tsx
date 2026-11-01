import { useEffect } from "react";
import { useCairn, useActions } from "../../app/cairnStore";

/** Follows the focused pane's active note into a live `/collab` presence
 *  session. Session lifecycle lives in collabSlice; this is the thin
 *  subscription (mirrors AskPanelHost). Presence itself now renders via
 *  PresenceCluster in the TopBar — this host owns only the follow/stop
 *  lifecycle and renders nothing. */
export function CollabPresenceHost() {
  const activePath = useCairn((s) => s.activePath);
  const actions = useActions();

  useEffect(() => {
    if (activePath) actions.collabFollow(activePath);
    else actions.collabStop();
  }, [activePath, actions]);

  // Leave the session when the app tears down.
  useEffect(() => () => actions.collabStop(), [actions]);

  return null;
}
