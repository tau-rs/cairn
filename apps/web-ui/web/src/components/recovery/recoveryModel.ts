import type { WireRecoverableBlock, WireBlockId } from "../../contract";

export type RecoveryKind = "deleted" | "overwritten";

export interface RecoveryItem {
  id: WireBlockId;
  kind: RecoveryKind;
  versions: string[];
}

/** Wire blocks → view items: drop empty-string versions, drop now-empty
 *  blocks, tag kind by tombstone. */
export function toRecoveryItems(
  blocks: WireRecoverableBlock[],
): RecoveryItem[] {
  const items: RecoveryItem[] = [];
  for (const b of blocks) {
    const versions = b.versions.filter((v) => v !== "");
    if (versions.length === 0) continue;
    items.push({
      id: b.id,
      kind: b.tombstoned ? "deleted" : "overwritten",
      versions,
    });
  }
  return items;
}

/** `#<replica>·<counter>` — the only locator (no live-doc correlation). */
export function blockLabel(id: WireBlockId): string {
  return `#${id.replica}·${id.counter}`;
}
