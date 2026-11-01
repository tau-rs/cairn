import { describe, it, expect } from "vitest";
import { toRecoveryItems, blockLabel } from "./recoveryModel";
import type { WireRecoverableBlock } from "../../contract";

const blk = (
  replica: number,
  counter: number,
  tombstoned: boolean,
  versions: string[],
): WireRecoverableBlock => ({
  id: { replica: String(replica), counter: String(counter) },
  tombstoned,
  versions,
});

describe("toRecoveryItems", () => {
  it("maps tombstoned→deleted and live→overwritten", () => {
    const items = toRecoveryItems([
      blk(1, 2, true, ["x"]),
      blk(1, 3, false, ["y"]),
    ]);
    expect(items.map((i) => i.kind)).toEqual(["deleted", "overwritten"]);
  });
  it("drops empty-string versions", () => {
    expect(
      toRecoveryItems([blk(1, 2, true, ["", "keep", ""])])[0].versions,
    ).toEqual(["keep"]);
  });
  it("drops a block whose only version is empty", () => {
    expect(toRecoveryItems([blk(1, 2, true, [""])])).toEqual([]);
  });
  it("blockLabel formats id", () => {
    expect(
      blockLabel({
        replica: "7",
        counter: "142",
      }),
    ).toBe("#7·142");
  });
});
