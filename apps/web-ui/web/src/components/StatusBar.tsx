import type { Revision } from "../contract/Revision";
import { relativeTime } from "./history/formatRevision";
import { versionWordDelta } from "./history/versionSummary";

export type SyncStatus = "ok" | "reconnecting" | "down";

/** Persistent bottom strip: the calm vault-wide home for the save / sync /
 *  version axes (spec Part 2 "Status bar"). Hidden on mobile (bottom-nav tier).
 *  "Saved" is strictly the disk-flush axis; the git layer is "Versions". */
export function StatusBar(props: {
  saving: boolean;
  dirty: boolean;
  sync: SyncStatus;
  lastVersion: Revision | null;
  onShowVersions: () => void;
}) {
  const saveLabel = props.saving
    ? "Saving…"
    : props.dirty
      ? "Unsaved changes"
      : "✓ Saved";
  const syncLabel =
    props.sync === "ok"
      ? "Synced"
      : props.sync === "reconnecting"
        ? "Syncing…"
        : "Offline — changes saved locally";
  const lv = props.lastVersion;
  return (
    <div
      data-testid="status-bar"
      className="hidden items-center gap-2 border-t border-border bg-surface px-3 py-1 text-xs text-muted md:flex"
    >
      <span data-testid="status-saved">{saveLabel}</span>
      <span aria-hidden>·</span>
      <span
        data-testid="status-sync"
        className={props.sync === "down" ? "font-medium text-text" : undefined}
      >
        {syncLabel}
      </span>
      <span aria-hidden>·</span>
      <button
        type="button"
        className="rounded px-1 hover:bg-surface-2 hover:text-text"
        onClick={props.onShowVersions}
      >
        🕘 Versions
      </button>
      <span className="grow" />
      {lv && (
        <span data-testid="status-last-version">
          Last version: {relativeTime(lv.timestamp_secs)}
          {(() => {
            const d = versionWordDelta(lv);
            return d && d.added > 0 ? ` · +${d.added} words` : "";
          })()}
        </span>
      )}
    </div>
  );
}
