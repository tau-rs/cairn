import { describe, it, expect } from "vitest";
import { groupRevisions, dayLabel, SESSION_GAP_SECS } from "./groupRevisions";
import type { Revision } from "../../contract/Revision";

// Fixed "now": 2026-08-21 12:00 local — tests build timestamps relative to it.
const NOW = new Date(2026, 7, 21, 12, 0, 0).getTime() / 1000;
const rev = (id: string, ts: number): Revision => ({
  id,
  message: `m-${id}`,
  author: "a",
  timestamp_secs: ts,
  summary: null,
  name: null,
});

describe("dayLabel", () => {
  it("labels today, yesterday and older dates", () => {
    expect(dayLabel(NOW - 60, NOW)).toBe("Today");
    expect(dayLabel(NOW - 86_400, NOW)).toBe("Yesterday");
    expect(dayLabel(NOW - 3 * 86_400, NOW)).not.toMatch(/Today|Yesterday/);
  });
});

describe("groupRevisions", () => {
  it("groups newest-first revisions into days and 30-min sessions", () => {
    const revs = [
      rev("c4", NOW - 60), // today, session A
      rev("c3", NOW - 60 - SESSION_GAP_SECS / 2), // today, session A (gap 15m)
      rev("c2", NOW - 60 - SESSION_GAP_SECS * 3), // today, session B (gap > 30m)
      rev("c1", NOW - 86_400), // yesterday
    ];
    const days = groupRevisions(revs, NOW);
    expect(days.map((d) => d.label)).toEqual(["Today", "Yesterday"]);
    expect(days[0].sessions).toHaveLength(2);
    expect(days[0].sessions[0].head.id).toBe("c4");
    expect(days[0].sessions[0].rest.map((r) => r.id)).toEqual(["c3"]);
    expect(days[0].sessions[1].head.id).toBe("c2");
    expect(days[1].sessions[0].head.id).toBe("c1");
  });

  it("returns [] for no revisions", () => {
    expect(groupRevisions([], NOW)).toEqual([]);
  });
});
