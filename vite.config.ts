/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      // elkjs's sync entry (lib/main.js, only reached when no Worker
      // global exists, e.g. the Vitest/Node test runner — see
      // src/layout/elk.ts) require()s the optional "web-worker" package,
      // which isn't a dependency and is never reached in the browser.
      // Point it at an empty shim so Vite's dev-time dependency scan
      // doesn't fail trying to resolve it.
      'web-worker': fileURLToPath(new URL('./src/shims/web-worker.ts', import.meta.url)),
    },
  },
  optimizeDeps: {
    // elk-api.js is a CJS/UMD bundle; the dev server must pre-bundle it to
    // get a working `default` export (the production build goes through
    // Rollup's CJS interop and never hits this).
    include: ['elkjs/lib/elk-api.js'],
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test-setup.ts'],
  },
});
