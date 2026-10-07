// Shim for the optional "web-worker" package, which elkjs's sync entry
// (lib/main.js) require()s but we never reach in the browser (we always
// construct ELK via elk-worker.js when Worker exists — see
// src/layout/elk.ts). Never actually called; exists only so Vite's
// dev-time dependency scan can resolve the import path.
export default class Worker {
  constructor() {
    throw new Error('web-worker shim: should never be constructed in the browser');
  }
}
