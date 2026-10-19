import assert from "node:assert/strict";
import { test } from "node:test";
import { stripTerminalSequences } from "@earendil-works/pi-tui";
import { renderContext } from "../lib/adapter.ts";
import type { RenderedMessage, Theme } from "../lib/pi.ts";
import { contextUpdate, type ContextRecord, type PendingPlan, type PlanEvent } from "../lib/updates.ts";
import { Fixture, ID, OTHER } from "./fake.ts";

/** commands.md §15.7 "Display" (hs-d221): one dim line of counts collapsed, the full text expanded, the model's `content` untouched. */

const theme: Theme = { fg: (color, text) => `<${color}>${text}</${color}>`, bg: (_color, text) => text };
const WIDTH = 200;
const render = (message: RenderedMessage, expanded: boolean) => renderContext(message, { expanded, outputPad: 1 }, theme)!.render(WIDTH).map((l) => stripTerminalSequences(l).trimEnd());

const record = (n: number): ContextRecord => ({ seq: 40 + n, id: `hs-n${n}`, kind: "note", at: "2026-10-17T09:00:00.000Z", by: OTHER, text: `note ${n}` });
const planEvent = (n: number): PlanEvent => ({ event: 300 + n, at: "2026-10-18T09:00:00Z", plan: "pay-4521", text: `Plan phase ${n}.` });
const pending: PendingPlan[] = [{ id: "pay-4521", phase: "dispatch", prs: [{ key: "api", status: "dispatch", number: 212, jobs: [{ key: "j1", status: "queued", title: "T" }] }] }];

const message = () => contextUpdate("ctx", ID, [1, 2].map((n) => ({ event: 200 + n, record: `hs-n${n}`, seq: 40 + n })), [record(1), record(2)], [planEvent(1), planEvent(2), planEvent(3)], pending)!;
const FULL = [
  "Context ctx: 2 new records since your last turn.",
  "- hs-n1 note (2026-10-17): note 1",
  "- hs-n2 note (2026-10-17): note 2",
  "Plans:",
  "- 2026-10-18T09:00:00Z pay-4521: Plan phase 1.",
  "- 2026-10-18T09:00:00Z pay-4521: Plan phase 2.",
  "- 2026-10-18T09:00:00Z pay-4521: Plan phase 3.",
  "Pending work:",
  "- pay-4521 (dispatch), PR api dispatch #212: 0 merged on its Lead Branch, 1 queued, 0 running, 0 failed, 0 draft",
  "  - j1 queued: T",
];

test("collapsed: one dim line of counts, not the full text", () => {
  assert.deepEqual(render(message(), false), [" <dim>cpd-context: 2 records · 3 Plan Events · 1 Plan pending</dim>"]);
});

test("expanded: the full text", () => {
  assert.deepEqual(render(message(), true), FULL.map((l) => ` ${l}`));
});

test("a message whose details lack the counts (an older adapter's) is shown in full, collapsed too", () => {
  const old = { ...message(), details: { event: 1, delivered_to: ID, records: ["hs-n1"] } };
  assert.deepEqual(render(old, false), FULL.map((l) => ` ${l}`));
});

test("the message's content is the full text, whatever the display", () => {
  const m = message();
  assert.equal(m.content, FULL.join("\n"));
  render(m, false);
  render(m, true);
  assert.equal(m.content, FULL.join("\n"));
  assert.equal(m.display, true);
});

test("the adapter registers the renderer for cpd-context once at load, for every role", () => {
  const f = new Fixture();
  try {
    f.load();
    assert.deepEqual([...f.pi.renderers.keys()], ["cpd-context"]);
    assert.equal(f.pi.renderers.get("cpd-context"), renderContext);
  } finally {
    f.close();
  }
});
