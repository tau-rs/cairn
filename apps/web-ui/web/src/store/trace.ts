/**
 * Observability seam for the index-refresh cascade. The subscribe handler fans
 * queries out on every `note_changed`; in dev we want to see which trigger
 * dispatched which actions and how long the backend calls took, so a refresh
 * storm (D1/D2) is visible. In production this is a no-op — no logging ships.
 */
export interface RefreshTrace {
  /** Record a refresh trigger: event type -> the actions it dispatched. */
  event(type: string, actions: string[]): void;
  /** Time an async backend call, returning its result unchanged. */
  time<T>(label: string, fn: () => Promise<T>): Promise<T>;
}

/** Production / test default: records nothing, just runs the thunk. */
export const noopTrace: RefreshTrace = {
  event: () => {},
  time: (_label, fn) => fn(),
};

/** Dev tracer: logs the fan-out and times each backend call via `console.debug`. */
export function makeConsoleTrace(): RefreshTrace {
  return {
    event: (type, actions) =>
      console.debug(`[cairn] refresh ← ${type}`, actions),
    time: async (label, fn) => {
      const t0 = performance.now();
      try {
        return await fn();
      } finally {
        console.debug(
          `[cairn] ${label} took ${(performance.now() - t0).toFixed(1)}ms`,
        );
      }
    },
  };
}

/** The singleton the store uses by default: dev tracer in dev, no-op in prod. */
export const refreshTrace: RefreshTrace = import.meta.env.DEV
  ? makeConsoleTrace()
  : noopTrace;
