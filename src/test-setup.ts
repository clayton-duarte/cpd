import { afterEach } from 'vitest';
import { cleanup } from '@testing-library/react';

afterEach(() => {
  cleanup();
});

if (typeof window !== 'undefined' && !window.matchMedia) {
  window.matchMedia = (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  });
}

// jsdom has no ResizeObserver; @xyflow/react's <ReactFlow> observes its
// container on mount. A no-op stub is sufficient for unit tests (C4: Jobs
// level renders inside jsdom via the App navigation tests).
if (typeof globalThis.ResizeObserver === 'undefined') {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}

// jsdom has no document.fonts (FontFaceSet). Mantine's autosizing Textarea
// listens for "loadingdone" to recompute height when webfonts finish
// loading; a no-op stub is enough for unit tests.
if (typeof document !== 'undefined' && !document.fonts) {
  Object.defineProperty(document, 'fonts', {
    value: {
      addEventListener: () => {},
      removeEventListener: () => {},
    },
    configurable: true,
  });
}

// jsdom has no EventSource. App.navigation.test.tsx mounts the full App,
// which now includes ChatPanel/useEngineStream at the Leads level; a no-op
// stub lets those tests mount without exercising SSE at all (that behavior
// is covered directly in useEngineStream.test.ts with a fake EventSource).
if (typeof globalThis.EventSource === 'undefined') {
  globalThis.EventSource = class {
    onopen: (() => void) | null = null;
    onerror: (() => void) | null = null;
    onmessage: ((e: MessageEvent) => void) | null = null;
    close() {}
  } as unknown as typeof EventSource;
}
