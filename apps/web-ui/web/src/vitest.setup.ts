import "@testing-library/jest-dom/vitest";

// Node 26 introduces an experimental `localStorage` accessor on `globalThis`
// that returns `undefined` unless `--localstorage-file` is set. This shadows
// jsdom's `localStorage` because Vitest's `populateGlobal` skips keys that
// already exist in the global (and `localStorage` isn't in its hardcoded allow-
// list). Restore jsdom's implementations by reading them from the real jsdom
// window (accessible via `globalThis.jsdom`) rather than the Vitest global
// (where `window === globalThis` and therefore carries Node 26's broken value).
const _jsdomWindow = (globalThis as { jsdom?: { window: Window } }).jsdom
  ?.window;
if (_jsdomWindow !== undefined) {
  Object.defineProperty(globalThis, "localStorage", {
    value: _jsdomWindow.localStorage,
    writable: true,
    configurable: true,
  });
  Object.defineProperty(globalThis, "sessionStorage", {
    value: _jsdomWindow.sessionStorage,
    writable: true,
    configurable: true,
  });
}

// jsdom lacks these APIs that Radix's focus / dismissable-layer use in tests.
if (!Element.prototype.hasPointerCapture) {
  Element.prototype.hasPointerCapture = () => false;
}
if (!Element.prototype.releasePointerCapture) {
  Element.prototype.releasePointerCapture = () => {};
}
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}
if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
}

if (typeof window !== "undefined" && typeof window.matchMedia !== "function") {
  window.matchMedia = ((query: string) => ({
    matches: true, // default to the largest tier (desktop) in tests
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}
