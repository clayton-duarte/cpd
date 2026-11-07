import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { readView, tag } from "../lib/origin.ts";
import { Fixture, planShow } from "./fake.ts";

/**
 * commands.md §15.3 "The user-message tag" (Plan ui-origin): the view reader and the tag formatter, the adapter's
 * interactive-input tag and the image-only rule. The marker is written to a temporary `$CPD_HOME` (never the real
 * User's), and only the adapter under test runs.
 */

/** A `$CPD_HOME` holding `run/ui-view.json`; `marker` undefined leaves the file absent, a string is written verbatim. */
function viewHome(marker: unknown): string {
  const home = mkdtempSync(join(tmpdir(), "cpd-origin-"));
  if (marker !== undefined) {
    mkdirSync(join(home, "run"), { recursive: true });
    writeFileSync(join(home, "run", "ui-view.json"), typeof marker === "string" ? marker : JSON.stringify(marker));
  }
  return home;
}

/** A pid that is not a live process: beyond both macOS's and Linux's `pid_max`. */
const DEAD_PID = 2147483646;

test("the tag renders via TUI with view WebUI", (t) => {
  const home = viewHome({ view: "webui", pid: process.pid });
  t.after(() => rmSync(home, { recursive: true, force: true }));
  assert.equal(tag("tui", readView(home)), "[via TUI · view: WebUI] ");
});

test("the tag renders via WebUI with view unknown when the marker is absent", (t) => {
  const home = viewHome(undefined);
  t.after(() => rmSync(home, { recursive: true, force: true }));
  assert.equal(tag("webui", readView(home)), "[via WebUI · view: unknown] ");
});

test("the tag renders WebUI Preview", (t) => {
  const home = viewHome({ view: "webui-preview", pid: process.pid });
  t.after(() => rmSync(home, { recursive: true, force: true }));
  assert.equal(tag("tui", readView(home)), "[via TUI · view: WebUI Preview] ");
});

test("a marker with a dead pid reads view unknown", (t) => {
  const dead = viewHome({ view: "webui", pid: DEAD_PID });
  const live = viewHome({ view: "webui", pid: process.pid });
  t.after(() => {
    rmSync(dead, { recursive: true, force: true });
    rmSync(live, { recursive: true, force: true });
  });
  assert.equal(readView(dead), "unknown");
  assert.equal(readView(live), "WebUI", "the control: a live pid reads its view");
});

/** A fixture whose `$CPD_HOME` carries a live view marker, with the adapter loaded. */
function loading(t: { after(fn: () => void): void }, view: "tui" | "webui" | "webui-preview"): Fixture {
  const f = new Fixture();
  f.respond("plan-show", planShow());
  f.load();
  mkdirSync(join(f.cpdHome, "run"), { recursive: true });
  writeFileSync(join(f.cpdHome, "run", "ui-view.json"), JSON.stringify({ view, pid: process.pid }));
  t.after(() => f.close());
  return f;
}

/** The handler's transform result of an `input` event, or undefined when no handler transformed the text. */
const transformOf = (results: unknown[]): { text: string; images?: unknown[] } | undefined =>
  results.find((r): r is { text: string; images?: unknown[] } => typeof r === "object" && r !== null && (r as { action?: unknown }).action === "transform");

test("interactive input is prefixed with the tag", async (t) => {
  const f = loading(t, "webui");
  const results = await f.pi.fireWith("input", { source: "interactive", text: "hello" });
  const r = transformOf(results);
  assert.ok(r, "the interactive input handler transforms the prompt");
  assert.equal(r.text, "[via TUI · view: WebUI] hello");
});

test("slash input is passed through untagged", async (t) => {
  const f = loading(t, "webui");
  const slash = await f.pi.fireWith("input", { source: "interactive", text: "/skill:explore" });
  assert.equal(transformOf(slash), undefined, "command text is not rewritten (skill/template expansion depends on the prefix)");
  const plain = await f.pi.fireWith("input", { source: "interactive", text: "hello" });
  assert.ok(transformOf(plain), "the control: plain interactive text is tagged");
});

test("non-interactive input is untagged", async (t) => {
  const f = loading(t, "webui");
  for (const source of ["extension", "rpc"]) {
    const results = await f.pi.fireWith("input", { source, text: "hello" });
    assert.equal(transformOf(results), undefined, `${source} input is left alone`);
  }
  const plain = await f.pi.fireWith("input", { source: "interactive", text: "hello" });
  assert.ok(transformOf(plain), "the control: interactive text is tagged");
});

test("an image-only interactive input is tagged and keeps its image", async (t) => {
  const f = loading(t, "webui");
  const image = { type: "image", data: "aGk=", mimeType: "image/png" };
  const event = { source: "interactive", text: "", images: [image] };
  const results = await f.pi.fireWith("input", event);
  const r = transformOf(results);
  assert.ok(r, "an image with no text is still a transform");
  assert.equal(r.text, "[via TUI · view: WebUI] ");
  // A transform may replace the images or omit them (pi keeps the current ones); either way the image survives.
  assert.deepEqual(r.images ?? event.images, [image]);
});
