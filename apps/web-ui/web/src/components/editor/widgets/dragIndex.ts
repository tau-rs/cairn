/** Given pointer position `p` along an axis and the sorted center offsets of each
 *  item, return the index the dragged item should land at. */
export function dropIndex(p: number, centers: number[]): number {
  let i = 0;
  while (i < centers.length && p > centers[i]) i++;
  return i;
}

/** The final move-to index for a grip dragged to pointer position `p`, given the
 *  pre-removal item `centers` and the dragged item's current `fromIndex`. Adjusts
 *  for moveRow/moveColumn's remove-then-insert semantics (downward drags shift the
 *  target left by one) and clamps to a valid index. */
export function dropTarget(
  p: number,
  centers: number[],
  fromIndex: number,
): number {
  let to = dropIndex(p, centers);
  if (to > fromIndex) to -= 1; // account for the removed slot on downward moves
  return Math.max(0, Math.min(centers.length - 1, to));
}
