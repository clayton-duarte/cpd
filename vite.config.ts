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
  server: {
    port: Number(process.env.CPD_DEV_PORT ?? 5173),
    proxy: {
      '/api': {
        target: process.env.CPD_DAEMON_URL ?? `http://localhost:${process.env.CPD_DAEMON_PORT ?? 4317}`,
        changeOrigin: true,
      },
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test-setup.ts'],
    // Git worktrees live in .worktrees/ and each contains a full copy of
    // src/, including that branch's own tests. Without this exclude the
    // runner picks up every sibling branch's suite and reports failures
    // that belong to other cards' work-in-progress.
    exclude: ['**/node_modules/**', '**/dist/**', '.worktrees/**', 'daemon/**'],
  },
});
