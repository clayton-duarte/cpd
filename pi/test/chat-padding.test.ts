/**
 * The uniform 1-cell chat-slot pad (decision hs-d684 item 3, docs/research/chat-padding.md): `paddedRoot` insets the
 * pi pane's fullscreen layout root by one blank cell on all four sides, and the adapter installs it as the `cpd-pad`
 * widget (`placement: "belowEditor"`) instead of the ad-hoc `cpd-pad` status and header.
 */
import assert from "node:assert/strict";
import { test } from "node:test";
import type { Component } from "@earendil-works/pi-tui";
import { Fixture, planShow, waitFor } from "./fake.ts";
import { paddedRoot } from "../lib/pad.ts";

const KEY = "cpd-pad";

test("paddedRoot insets its root by one blank cell on all four sides", () => {
  const fake: Component = { render: (width: number) => ["R".repeat(width)], invalidate() {} };
  assert.deepEqual(paddedRoot(fake).render(5), ["     ", " RRR ", "     "]);
});

test("the adapter installs the chat pad widget instead of the cpd-pad status and header", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  const widgets: { key: string; factory: unknown; options: unknown }[] = [];
  const statuses: { key: string; text: string | undefined }[] = [];
  const headers: unknown[] = [];
  const base = f.pi.ctx.bind(f.pi);
  f.pi.ctx = () => {
    const ctx = base();
    const ui = ctx.ui as unknown as Record<string, unknown>;
    // pi's `ctx.ui.setWidget(key, factory, options)`: installs a widget, `undefined` removes it.
    ui.setWidget = (key: string, factory: unknown, options: unknown) => {
      widgets.push({ key, factory, options });
    };
    ui.setStatus = (key: string, text: string | undefined) => {
      statuses.push({ key, text });
    };
    ui.setHeader = (factory: unknown) => {
      headers.push(factory);
    };
    return ctx;
  };

  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);

  assert.deepEqual(widgets.map((w) => w.key), [KEY], "once, at session_start");
  assert.deepEqual(widgets[0]?.options, { placement: "belowEditor" }, "below the editor, the chat slot's pad");
  assert.deepEqual(statuses, [], "the cpd-pad status is not set");
  assert.deepEqual(headers, [], "no header is installed");

  // /reload: a new session_start installs the widget again; never doubled
  await f.pi.fire("session_start", { reason: "reload" });
  assert.deepEqual(widgets.map((w) => w.key), [KEY, KEY], "set again at the reload, never doubled");
  assert.deepEqual(statuses, [], "still no status after the reload");
  assert.deepEqual(headers, [], "still no header after the reload");
});
