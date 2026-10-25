/**
 * The chat-slot pad (docs/contract.md section 6.1, decision hs-d684 item 3): the adapter no longer sets the `cpd-pad`
 * status or a header for the blank rows; the uniform 1-cell pad is installed as the `cpd-pad` widget
 * (`placement: "belowEditor"`), so pi's own footer and header are left untouched.
 */
import assert from "node:assert/strict";
import { test } from "node:test";
import { Fixture, planShow, waitFor } from "./fake.ts";

const KEY = "cpd-pad";

test("the adapter installs the cpd-pad widget and leaves the footer and header alone", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  const widgets: { key: string; options: unknown }[] = [];
  const statuses: unknown[] = [];
  const headers: unknown[] = [];
  const base = f.pi.ctx.bind(f.pi);
  f.pi.ctx = () => {
    const ctx = base();
    const ui = ctx.ui as unknown as Record<string, unknown>;
    ui.setWidget = (key: string, _factory: unknown, options: unknown) => {
      widgets.push({ key, options });
    };
    ui.setStatus = (...args: unknown[]) => {
      statuses.push(args);
    };
    ui.setHeader = (factory: unknown) => {
      headers.push(factory);
    };
    return ctx;
  };

  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);

  assert.deepEqual(widgets, [{ key: KEY, options: { placement: "belowEditor" } }], "once, at session_start");
  assert.deepEqual(statuses, [], "the cpd-pad status is not set");
  assert.deepEqual(headers, [], "no header is installed");

  // /reload: a new session_start installs the widget again; the old status/header stay untouched
  await f.pi.fire("session_start", { reason: "reload" });
  assert.deepEqual(widgets.map((w) => w.key), [KEY, KEY], "set again at the reload");
  assert.deepEqual(statuses, []);
  assert.deepEqual(headers, []);
});
