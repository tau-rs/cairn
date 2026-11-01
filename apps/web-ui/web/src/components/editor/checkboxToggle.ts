/** Given the doc text and the index of the `[` in a `[ ]`/`[x]` task marker,
 *  return the single-character change that flips its state. */
export function toggleCheckboxChange(
  doc: string,
  bracketOpen: number,
): { from: number; to: number; insert: string } {
  const inner = doc[bracketOpen + 1];
  const insert = inner === " " ? "x" : " ";
  return { from: bracketOpen + 1, to: bracketOpen + 2, insert };
}
