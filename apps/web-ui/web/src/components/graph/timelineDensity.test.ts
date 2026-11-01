import { describe, it, expect } from "vitest";
import { timelineBuckets, describeSelection } from "./timelineDensity";
import type { Revision } from "../../contract";

const rev = (id: string, t: number, msg = id): Revision => ({
  id,
  message: msg,
  timestamp_secs: t,
  author: "x",
  summary: null,
  name: null,
});

describe("timelineBuckets", () => {
  it("returns [] for an empty timeline", () => {
    expect(timelineBuckets([])).toEqual([]);
  });

  it("distributes revisions across buckets by timestamp; counts sum to N", () => {
    const revs = [rev("a", 0), rev("b", 50), rev("c", 100)];
    const buckets = timelineBuckets(revs, 10);
    expect(buckets).toHaveLength(10);
    expect(buckets.reduce((s, b) => s + b.count, 0)).toBe(3);
    expect(buckets[0].count).toBe(1); // oldest
    expect(buckets[9].count).toBe(1); // newest lands in the last bucket
  });

  it("puts everything in bucket 0 when all timestamps are equal (no div-by-zero)", () => {
    const revs = [rev("a", 5), rev("b", 5)];
    const buckets = timelineBuckets(revs, 4);
    expect(buckets).toHaveLength(4);
    expect(buckets[0].count).toBe(2);
  });

  it("does not throw and stays a fixed 12-length histogram at scale (200k revisions)", () => {
    const N = 200_000;
    const big: Revision[] = Array.from({ length: N }, (_, i) =>
      rev(String(i), i),
    );
    let buckets: { count: number }[] = [];
    expect(() => {
      buckets = timelineBuckets(big);
    }).not.toThrow();
    expect(buckets).toHaveLength(12);
    expect(buckets.reduce((s, b) => s + b.count, 0)).toBe(N);
  });
});

describe("describeSelection", () => {
  const tl = [rev("r2", 20, "add b"), rev("r1", 10, "init")]; // newest-first

  it("describes live", () => {
    expect(describeSelection({ kind: "live" }, tl)).toEqual({
      state: "Live",
      detail: "current vault",
    });
  });

  it("describes a snapshot with date + message", () => {
    const d = describeSelection({ kind: "snapshot", at: 1 }, tl); // r1
    expect(d.state).toBe("Viewing vault as of");
    expect(d.detail).toContain("init");
    expect(d.detail).toContain("1970-01-01");
  });

  it("describes a compare range", () => {
    const d = describeSelection({ kind: "compare", from: 1, to: 0 }, tl);
    expect(d.state).toBe("Comparing");
    expect(d.detail).toContain("→");
  });

  it("falls back to Live on an out-of-range index", () => {
    expect(describeSelection({ kind: "snapshot", at: 9 }, tl).state).toBe(
      "Live",
    );
  });
});
