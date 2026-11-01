import { useNavigate } from "react-router-dom";
import { useCairn, useActions } from "../../app/cairnStore";
import { noteUrl } from "../../app/routes";
import { AskSheet } from "./AskSheet";
import { resolveStem } from "./citation";

/** Wires AskSheet to the store + router for the small-screen shells. `side` is
 *  supplied by the shell (mobile = bottom, tablet = right). Shows the sheet only
 *  when the conversation is in panel mode. */
export function AskSheetHost({ side }: { side: "bottom" | "right" }) {
  const navigate = useNavigate();
  const actions = useActions();
  const ask = useCairn((s) => s.ask);
  const notePaths = useCairn((s) => s.notePaths);

  return (
    <AskSheet
      open={ask.mode === "panel"}
      side={side}
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
