/**
 * The empty last row of the pi pane (docs/contract.md section 6.1, "The blank row", rule Q2a/Q3): at `session_start` the adapter calls
 * `ctx.ui.setStatus("cpd-pad", "")`, which makes pi's real footer draw one more, empty, line.
 */
import assert from "node:assert/strict";
import { test } from "node:test";
import { FooterComponent, initTheme } from "@earendil-works/pi-coding-agent";
import { Fixture, planShow, waitFor } from "./fake.ts";

const KEY = "cpd-pad";
const SGR = /\x1b\[[0-9;]*m/g;

/** The footer-data provider pi hands the footer: the extension statuses are what `ctx.ui.setStatus` recorded. */
const statuses = new Map<string, string>();
const footerData = {
  getGitBranch: () => null,
  getAvailableProviderCount: () => 1,
  getExtensionStatuses: () => statuses,
};
const session = {
  state: { model: undefined, thinkingLevel: "off" },
  routedModel: undefined,
  model: undefined,
  modelRuntime: { isUsingSubscription: () => false },
  getContextUsage: () => undefined,
  sessionManager: {
    getEntryCount: () => 0,
    getSessionId: () => "s",
    getLeafId: () => null,
    getEntries: () => [],
    getCwd: () => "/work/dir",
    getSessionName: () => undefined,
  },
};
const render = (width: number): string[] => new FooterComponent(session as never, footerData as never).render(width).map((l) => l.replace(SGR, ""));

test("the adapter leaves one empty row at the bottom of the pi screen", async (t) => {
  initTheme();
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  const calls: { key: string; text: string | undefined }[] = [];
  const base = f.pi.ctx.bind(f.pi);
  f.pi.ctx = () => {
    const ctx = base();
    // pi's `ctx.ui.setStatus(key, text)`: `undefined` deletes the entry, anything else (even "") sets it.
    (ctx.ui as unknown as Record<string, unknown>).setStatus = (key: string, text: string | undefined) => {
      calls.push({ key, text });
      if (text === undefined) statuses.delete(key);
      else statuses.set(key, text);
    };
    return ctx;
  };
  statuses.clear();
  statuses.set("other-ext", "KEEP");
  const before = render(60);
  assert.equal(before.length, 3, "control: the footer has two lines and the other extension's status line");

  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);

  assert.deepEqual(calls.filter((c) => c.key === KEY), [{ key: KEY, text: "" }], "once, at session_start, with the empty text");
  assert.deepEqual(calls.filter((c) => c.key !== KEY), [], "no other entry is replaced or deleted");
  assert.equal(statuses.get("other-ext"), "KEEP");

  statuses.delete("other-ext");
  const lines = render(60);
  assert.equal(lines.length, 3, "the footer gains exactly one line");
  assert.equal(lines.at(-1), "", "its last line is empty");
  assert.deepEqual(lines.slice(0, 2), before.slice(0, 2), "the lines above are unchanged");

  // /reload: a new session_start sets the same entry again; still exactly one entry
  await f.pi.fire("session_start", { reason: "reload" });
  assert.deepEqual(calls.filter((c) => c.key === KEY).map((c) => c.text), ["", ""]);
  assert.deepEqual([...statuses.keys()], [KEY], "exactly one such entry after the second start");
  assert.equal(render(60).length, 3);
});

test("the adapter leaves one empty row at the top of the pi screen", async (t) => {
  initTheme();
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  // pi's `ctx.ui.setHeader(factory)`: installing a factory replaces the built-in header (contract §6.1 Q5).
  const headers: unknown[] = [];
  const base = f.pi.ctx.bind(f.pi);
  f.pi.ctx = () => {
    const ctx = base();
    (ctx.ui as unknown as Record<string, unknown>).setHeader = (factory: unknown) => {
      headers.push(factory);
    };
    return ctx;
  };

  type Header = { render(width: number): string[] };
  const headerLines = (factory: unknown): string[] => {
    assert.equal(typeof factory, "function", "the adapter installs a factory");
    return (factory as () => Header)().render(60).map((l) => l.replace(SGR, ""));
  };

  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);

  assert.equal(headers.length, 1, "once, at session_start");
  assert.deepEqual(headerLines(headers[0]), [""], "exactly one empty line at the top");

  // /reload: a new session_start replaces the previous header; still exactly one empty line, never two
  await f.pi.fire("session_start", { reason: "reload" });
  assert.equal(headers.length, 2, "set again at the reload");
  assert.deepEqual(headerLines(headers[1]), [""], "still exactly one empty line, never two");
});
