// The jsdom view tests' shared setup. jsdom does not implement ResizeObserver, which the
// AI Elements Conversation (`use-stick-to-bottom`) calls when its content mounts; a browser
// always has it. Define the minimal no-op observer only when it is missing, and nothing else:
// add a shim here only for a global a test actually proves missing.

if (typeof globalThis.ResizeObserver === "undefined") {
  class NoopResizeObserver implements ResizeObserver {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
  globalThis.ResizeObserver = NoopResizeObserver;
}

// jsdom does not implement `window.matchMedia` either; the vendored shadcn `use-mobile` hook
// calls it when the shell's Context rail mounts (web/src/hooks/use-mobile.ts), and this Gate run
// proves it missing. A browser always has it: define the minimal MediaQueryList only when it is
// missing. Its query never matches (the desktop rail, not the mobile sheet, is what renders).
if (typeof window.matchMedia !== "function") {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent() {
      return false;
    },
  })) as unknown as typeof window.matchMedia;
}
