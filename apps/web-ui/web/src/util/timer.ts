export interface Debounced {
  (): void;
  cancel(): void;
}

/**
 * Debounce a zero-arg function: invoke `fn` `ms` after the last call. `ms` may
 * be a thunk, re-read on every trigger — so a persistent debounce can pick up a
 * changed delay (e.g. an updated autosave interval) without being rebuilt.
 */
export function debounce(
  fn: () => void,
  ms: number | (() => number),
): Debounced {
  const delay = typeof ms === "function" ? ms : () => ms;
  let handle: ReturnType<typeof setTimeout> | null = null;
  const d = (() => {
    if (handle) clearTimeout(handle);
    handle = setTimeout(() => {
      handle = null;
      fn();
    }, delay());
  }) as Debounced;
  d.cancel = () => {
    if (handle) clearTimeout(handle);
    handle = null;
  };
  return d;
}
