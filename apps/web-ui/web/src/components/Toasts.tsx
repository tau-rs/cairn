import { useCairn, useActions } from "../app/cairnStore";
import { ErrorToast } from "./ErrorToast";
import { NoticeToast } from "./NoticeToast";

export function Toasts() {
  const actions = useActions();
  const errors = useCairn((s) => s.errors);
  const notice = useCairn((s) => s.notice);
  return (
    <>
      <ErrorToast errors={errors} onDismiss={actions.dismissError} />
      <NoticeToast message={notice} onDismiss={actions.dismissNotice} />
    </>
  );
}
