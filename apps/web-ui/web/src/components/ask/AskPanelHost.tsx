import { useNavigate } from "react-router-dom";
import { useCairn, useActions } from "../../app/cairnStore";
import { noteUrl } from "../../app/routes";
import { AskPanel } from "./AskPanel";
import { resolveStem } from "./citation";

/** Wires the docked AskPanel to the store + router. Passed as the `ask` shell
 *  region; renders null unless the conversation is in panel mode. */
export function AskPanelHost() {
  const navigate = useNavigate();
  const actions = useActions();
  const ask = useCairn((s) => s.ask);
  const notePaths = useCairn((s) => s.notePaths);

  return (
    <AskPanel
      open={ask.mode === "panel"}
      turns={ask.turns}
      streaming={ask.streaming}
      error={ask.error}
      onSubmit={actions.askSubmit}
      onClose={actions.askClose}
      onOpenNote={(target) => {
        const path = resolveStem(notePaths, target);
        if (path) navigate(noteUrl(path));
      }}
    />
  );
}
