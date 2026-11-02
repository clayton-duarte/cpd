import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { LEAD_MESSAGES } from "../lib/delivery.ts";
import { KINDS, parseLine } from "../lib/event.ts";
import { parsePrOpened } from "../lib/updates.ts";

/** The kinds of contract.md §5.2's table: the backticked names of the first column of its rows. */
function contractKinds(): string[] {
  const doc = readFileSync(new URL("../../docs/contract.md", import.meta.url), "utf8");
  const start = doc.indexOf("\n### 5.2 Kinds");
  const end = doc.indexOf("\n### 5.3", start);
  assert.ok(start >= 0 && end > start, "§5.2 is in docs/contract.md");
  const kinds: string[] = [];
  for (const line of doc.slice(start, end).split("\n")) {
    if (!line.startsWith("|")) continue;
    const first = line.split("|")[1]!.trim();
    if (first === "Kind" || /^-+$/.test(first)) continue;
    const names = [...first.matchAll(/`([a-z-]+)`/g)].map((m) => m[1]!);
    assert.ok(names.length > 0, `a kind in row: ${line.slice(0, 60)}`);
    kinds.push(...names);
  }
  return kinds;
}

test("KINDS is exactly the kinds of contract §5.2", () => {
  const contract = contractKinds();
  assert.equal(new Set(contract).size, contract.length, "§5.2 lists no kind twice");
  assert.deepEqual([...KINDS].sort(), [...contract].sort());
});

test("every key of LEAD_MESSAGES is a known kind", () => {
  for (const k of Object.keys(LEAD_MESSAGES)) assert.ok((KINDS as readonly string[]).includes(k), k);
});

test("session-titled is a known kind the adapter delivers as no message and no turn (hs-d257)", () => {
  const line = { id: 9, at: "t", kind: "session-titled", context: "ctx", plan: "p", job: null, attempt: null, session: "s", text: 'Session s titled "T".', data: { title: "T" } };
  const parsed = parseLine(JSON.stringify(line));
  assert.equal(parsed.type, "event", "a known kind: not unknown-kind, which costs an error turn (hs-n122)");
  assert.ok(!("session-titled" in LEAD_MESSAGES), "no message, no turn");
});

const WELL_FORMED = { id: 7, at: "t", kind: "job-merged", context: null, plan: null, job: null, attempt: null, session: "s", text: "x", data: {} };

test("parseLine: a well-formed Event of an unknown kind is unknown-kind, not invalid", () => {
  assert.deepEqual(parseLine(JSON.stringify({ ...WELL_FORMED, kind: "from-the-future" })), { type: "unknown-kind", id: 7 });
  assert.equal(parseLine(JSON.stringify(WELL_FORMED)).type, "event");
});

test("reads the workflow kinds", () => {
  // contract §5.2 "Renamed kinds": the six kinds that name the Workflow.
  const workflowKinds = ["workflow-added", "workflow-edited", "workflow-described", "workflow-cancelled", "workflow-started", "workflow-base-changed"];
  for (const kind of workflowKinds) {
    assert.ok((KINDS as readonly string[]).includes(kind), `${kind} is a known kind`);
    assert.equal(parseLine(JSON.stringify({ ...WELL_FORMED, kind, data: { workflow: "w" } })).type, "event", `${kind} parses as an Event, not unknown-kind`);
  }

  // contract §5.2: the `data` key is `workflow`; the adapter still reads the legacy `data.pr` of `pr-opened` Events.
  const opened = { ...WELL_FORMED, plan: "p", kind: "pr-opened", data: { workflow: "w", number: 1, url: "u" } };
  const read = parsePrOpened({ events: [opened] });
  assert.equal(read.length, 1, "a pr-opened Event with data.workflow is read");
  assert.equal(read[0]!.event, 7);
  assert.equal(read[0]!.plan, "p");
  assert.equal(read[0]!.workflow, "w");
  assert.equal(read[0]!.number, 1);
  assert.equal(read[0]!.url, "u");
  const legacy = parsePrOpened({ events: [{ ...opened, data: { pr: "w", number: 1, url: "u" } }] });
  assert.equal(legacy.length, 1, "the legacy data.pr key is read");
  assert.equal(legacy[0]!.workflow, "w", "the legacy data.pr key is read");
  assert.equal(legacy[0]!.number, 1, "the legacy data.pr key is read");
  assert.equal(legacy[0]!.url, "u", "the legacy data.pr key is read");
  assert.throws(() => parsePrOpened({ events: [{ ...opened, data: { number: 1, url: "u" } }] }), { code: "bad_output" }, "an Event with neither data.workflow nor data.pr is refused");

  // An unknown kind is still unknown-kind, not invalid (contract §5.2).
  assert.deepEqual(parseLine(JSON.stringify({ ...WELL_FORMED, kind: "from-the-future" })), { type: "unknown-kind", id: 7 });
});

test("parseLine: a malformed line is invalid even with an unknown kind", () => {
  assert.equal(parseLine("nope").type, "invalid");
  assert.equal(parseLine(JSON.stringify({ ...WELL_FORMED, kind: "from-the-future", text: 1 })).type, "invalid");
  assert.equal(parseLine(JSON.stringify({ ...WELL_FORMED, kind: 3 })).type, "invalid");
  assert.equal(parseLine(JSON.stringify({ ...WELL_FORMED, kind: "from-the-future", id: 0 })).type, "invalid");
});
