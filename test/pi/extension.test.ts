import { test } from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";

// The adapter imports `typebox` (a pi dependency, not this repo's). Its schemas are only built inside event handlers, which
// these tests never fire, so a stub is enough to load the module. The specifier is a variable: pi/ is outside the type check.
registerHooks({
  resolve: (specifier, context, next) =>
    specifier === "typebox" ? { url: "data:text/javascript,export const Type = {}", shortCircuit: true } : next(specifier, context),
});
const extensionPath = "../../pi/extension.ts";
const extension: (pi: unknown) => void = (await import(extensionPath)).default;

/** Runs the extension with `env` and returns every `pi.*` call it made. */
function load(env: Record<string, string | undefined>): string[] {
  const calls: string[] = [];
  const pi = new Proxy({}, { get: (_t, name) => (...args: unknown[]) => { calls.push(`${String(name)}(${typeof args[0] === "string" ? args[0] : ""})`); } });
  const saved = { CPD_SESSION: process.env.CPD_SESSION, HERDR_ENV: process.env.HERDR_ENV, TMUX_PANE: process.env.TMUX_PANE };
  try {
    for (const k of Object.keys(saved)) delete process.env[k];
    Object.assign(process.env, env);
    extension(pi);
  } finally {
    for (const [k, v] of Object.entries(saved)) {
      if (v === undefined) delete process.env[k];
      else process.env[k] = v;
    }
  }
  return calls;
}

test("extension: with CPD_SESSION set (a cpd 2 session) it registers no tool and subscribes to no event, even in a herdr pane", () => {
  assert.deepEqual(load({ CPD_SESSION: "s1", HERDR_ENV: "1" }), []);
  assert.deepEqual(load({ CPD_SESSION: "s1", TMUX_PANE: "%1" }), []);
});

test("extension: without CPD_SESSION it still subscribes to session events in a herdr pane", () => {
  assert.deepEqual(load({ HERDR_ENV: "1" }), ["on(session_start)", "on(session_shutdown)"]);
  assert.deepEqual(load({ CPD_SESSION: "", HERDR_ENV: "1" }), ["on(session_start)", "on(session_shutdown)"]);
});

test("extension: outside a herdr or tmux pane it does nothing", () => {
  assert.deepEqual(load({}), []);
});
