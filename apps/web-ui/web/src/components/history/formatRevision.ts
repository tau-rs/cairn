/** Relative label like "5m ago" for a Unix-seconds timestamp.
 *  `nowSecs` is injectable for deterministic tests (defaults to wall clock). */
export function relativeTime(
  tsSecs: number,
  nowSecs: number = Math.floor(Date.now() / 1000),
): string {
  const delta = nowSecs - tsSecs;
  if (delta < 60) return "just now";
  if (delta < 3600) return `${Math.floor(delta / 60)}m ago`;
  if (delta < 86400) return `${Math.floor(delta / 3600)}h ago`;
  return `${Math.floor(delta / 86400)}d ago`;
}

/** Full locale date-time for tooltips. */
export function absoluteTime(tsSecs: number): string {
  return new Date(tsSecs * 1000).toLocaleString();
}
