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
