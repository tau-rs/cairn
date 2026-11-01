import { Button } from "../ui/Button";
import type { CollabPeer } from "../../store/collabSlice";

export function editingLabel(peers: CollabPeer[]): string {
  const names = peers
    .filter((p) => p.editing && p.name)
    .map((p) => p.name as string);
  if (names.length === 0) return "Someone is editing…";
  if (names.length === 1) return `${names[0]} is editing…`;
  if (names.length === 2) return `${names[0]}, ${names[1]} editing…`;
  return `${names[0]}, ${names[1]} +${names.length - 2} editing…`;
}

const CHIP =
  "flex items-center gap-2 rounded-full border border-border bg-surface-2 px-2.5 py-1 text-xs";

/** One persistent presence element in the top-right of the TopBar (spec Part 1).
 *  Calm by default, louder only when it matters; replaces the two corner cards.
 *  Priority: conflict > offline > reconnecting > live editing > baseline. */
export function PresenceCluster(props: {
  status: "ok" | "reconnecting" | "down";
  live: boolean;
  peers: CollabPeer[];
  conflictCount: number;
  onConflict: () => void;
  onReconnect: () => void;
}) {
  if (props.conflictCount > 0) {
    return (
      <button
        type="button"
        onClick={props.onConflict}
        className={`${CHIP} text-text hover:bg-surface`}
        data-testid="presence-conflict"
      >
        <span aria-hidden className="h-2 w-2 rounded-full bg-accent" />
        <span>Also changed on another device</span>
      </button>
    );
  }
  if (props.status === "down") {
    return (
      <div
        role="status"
        className={`${CHIP} text-text`}
        data-testid="presence-offline"
      >
        <span aria-hidden className="h-2 w-2 rounded-full bg-danger" />
        <span>Offline</span>
        <Button variant="ghost" onClick={props.onReconnect}>
          Reconnect
        </Button>
      </div>
    );
  }
  if (props.status === "reconnecting") {
    return (
      <div
        role="status"
        className={`${CHIP} text-muted`}
        data-testid="presence-reconnecting"
      >
        <span
          aria-hidden
          className="h-2 w-2 animate-pulse rounded-full bg-accent"
        />
        <span>Reconnecting…</span>
      </div>
    );
  }
  if (props.live) {
    const pips = Math.max(1, props.peers.filter((p) => p.editing).length);
    return (
      <div
        role="status"
        className={`${CHIP} text-muted`}
        data-testid="presence-editing"
      >
        {Array.from({ length: Math.min(pips, 3) }, (_, i) => (
          <span
            key={i}
            aria-hidden
            className="h-2 w-2 animate-pulse rounded-full bg-success"
          />
        ))}
        <span>{editingLabel(props.peers)}</span>
      </div>
    );
  }
  return (
    <div
      role="status"
      className={`${CHIP} text-muted`}
      data-testid="presence-idle"
    >
      <span aria-hidden className="h-2 w-2 rounded-full bg-success" />
      <span>
        {props.peers.length > 0 ? `${props.peers.length} here` : "Connected"}
      </span>
    </div>
  );
}
