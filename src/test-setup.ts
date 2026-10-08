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

// jsdom 30.1.2 does not provide window.localStorage (or the bare global
// `localStorage` identifier) in our configuration -- confirmed with a
// standalone probe on a clean checkout (`typeof localStorage === 'undefined'`
// and `window.localStorage === undefined`). A minimal in-memory Storage
// stub is sufficient for unit tests (L7b: the resizable chat panel persists
// its width here).
if (typeof globalThis.localStorage === 'undefined') {
  class MemoryStorage implements Storage {
    private store = new Map<string, string>();

    get length(): number {
      return this.store.size;
    }

    clear(): void {
      this.store.clear();
    }

    getItem(key: string): string | null {
      return this.store.has(key) ? this.store.get(key)! : null;
    }

    key(index: number): string | null {
      return Array.from(this.store.keys())[index] ?? null;
    }

    removeItem(key: string): void {
      this.store.delete(key);
    }

    setItem(key: string, value: string): void {
      this.store.set(key, String(value));
    }
  }

  const memoryStorage = new MemoryStorage();
  Object.defineProperty(globalThis, 'localStorage', {
    value: memoryStorage,
    configurable: true,
    writable: true,
  });
  if (typeof window !== 'undefined') {
    Object.defineProperty(window, 'localStorage', {
      value: memoryStorage,
      configurable: true,
      writable: true,
    });
  }
}
