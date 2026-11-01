import type { Revision } from "../../contract/Revision";

/** Two seals within this gap belong to the same editing session (spec: the
 *  browser tames the flat auto-stream by grouping, not deletion). */
export const SESSION_GAP_SECS = 30 * 60;

export interface SessionGroup {
  head: Revision;
  rest: Revision[];
}
export interface DayGroup {
  label: string;
  sessions: SessionGroup[];
}

export function dayLabel(tsSecs: number, nowSecs: number): string {
  const d = new Date(tsSecs * 1000);
  const n = new Date(nowSecs * 1000);
  const startOfDay = (x: Date) =>
    new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diffDays = Math.round((startOfDay(n) - startOfDay(d)) / 86_400_000);
  if (diffDays <= 0) return "Today";
  if (diffDays === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: d.getFullYear() === n.getFullYear() ? undefined : "numeric",
  });
}

/** Input is newest-first (contract order); output preserves it. */
export function groupRevisions(revs: Revision[], nowSecs: number): DayGroup[] {
  const days: DayGroup[] = [];
  for (const r of revs) {
    const label = dayLabel(r.timestamp_secs, nowSecs);
    let day = days[days.length - 1];
    if (!day || day.label !== label) {
      day = { label, sessions: [] };
      days.push(day);
    }
    const session = day.sessions[day.sessions.length - 1];
    const prev = session
      ? (session.rest[session.rest.length - 1] ?? session.head)
      : null;
    if (prev && prev.timestamp_secs - r.timestamp_secs <= SESSION_GAP_SECS) {
      session.rest.push(r);
    } else {
      day.sessions.push({ head: r, rest: [] });
    }
  }
  return days;
}
