/**
 * Install a window-level `unhandledrejection` handler so the codebase's liberal
 * `void promise` pattern fails loud: any rejection that escapes a store action's
 * own try/catch (a throw before its try block, or a future action that forgets
 * one) is reported instead of vanishing silently. Returns a disposer that
 * removes the listener.
 *
 * `sink` defaults to a structured `console.error`; callers may pass their own to
 * surface escaped rejections elsewhere.
 */
export function installGlobalRejectionHandler(
  sink: (reason: unknown) => void = (reason) =>
    console.error("[cairn] unhandled promise rejection", { error: reason }),
): () => void {
  const handler = (e: PromiseRejectionEvent) => sink(e.reason);
  window.addEventListener("unhandledrejection", handler);
  return () => window.removeEventListener("unhandledrejection", handler);
}
