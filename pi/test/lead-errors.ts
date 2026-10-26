import assert from "node:assert/strict";
import { type Fixture, ID } from "./fake.ts";

/**
 * A Lead's adapter errors (commands.md §15.1, hs-d573 cause A): not delivered to its chat (`f.pi.errors()` stays empty, asserted at every call) but reported with
 * `cpd session report` and written to the adapter log. The reports, each with the text `φ adapter: <what>: <code>: <message>` a chat message would have had.
 */
export function adapterErrors(f: Fixture) {
  assert.deepEqual(f.pi.errors(), [], "a Lead's adapter error is not delivered to its chat");
  return f.reports().map((r) => {
    assert.equal(r.session, ID);
    const line = `error: ${r.what}: ${r.code}: ${r.message}`.replace(/\s*\n\s*/g, " ");
    assert.ok(f.adapterLog().some((l) => l.endsWith(` ${line}`)), `not in the adapter log: ${line}`);
    return { what: r.what, code: r.code, message: { content: `φ adapter: ${r.what}: ${r.code}: ${r.message}` } };
  });
}
