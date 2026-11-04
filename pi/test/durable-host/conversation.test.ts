import assert from "node:assert/strict";
import { test } from "node:test";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { conversationOf as build } from "../../durable-worker/harness.ts";

type Status = { status: "done"; entry: string } | { status: "unanswered" } | { status: "failed" };

type Listener = (call: unknown, context: unknown) => Promise<void>;

/** A fake root: `submit` records its argument and returns a fake submission; no pi-durable store, no model, no timer. */
function rig(status: Status = { status: "done", entry: "e1" }, entries: readonly unknown[] = []) {
  const submitted: { input: unknown; ctx: unknown }[] = [];
  const root = {
    submit: async (input: unknown, ctx: unknown) => {
      submitted.push({ input, ctx });
      return {
        wait: async () => ({ status: status.status === "done" ? "done" : status.status }),
        status: async () => status,
        id: 7,
      };
    },
    context: async (ctx: unknown) => (contexts.push(ctx), { entries }),
    watch: async (...a: unknown[]) => (watched.push(a), stream),
    id: "conv1",
    abort: async (ctx: unknown, opts: unknown) => void aborts.push({ ctx, opts }),
  };
  const contexts: unknown[] = [];
  const aborts: { ctx: unknown; opts: unknown }[] = [];
  const stopped: string[] = [];
  const started: Listener[] = [];
  const end = { kind: "stopped" };
  const watched: unknown[][] = [];
  const stream = {
    value: { conversation: "conv1", entries: [], docs: {} },
    start: (l: Listener) => void started.push(l),
    stop: async () => void stopped.push("stop"),
    closed: Promise.resolve(end),
  };
  const abortedIds: unknown[][] = [];
  const harness = { abortSubmission: async (...a: unknown[]) => (abortedIds.push(a), "settled" as const) };
  const durable = {
    harness,
  };
  const installed: unknown[] = [];
  const registry = { install: (extension: unknown) => void installed.push(extension) };
  const closed = new Promise<unknown>(() => {});
  return { submitted, contexts, installed, closed, aborts, stopped, started, watched, abortedIds, harness, stream, end, conversation: build(root, registry, closed, durable as never) };
}

const ENTRY = { kind: "cpd-event", data: { id: 5, kind: "pr-feedback", delivered_to: "s1" } };

test("conversationOf submit passes the entry of the input to the root", async () => {
  const r = rig();
  await r.conversation.submit({ content: "hello", requestId: "event:5", whenBusy: "steer", entry: ENTRY });
  assert.deepEqual(r.submitted.map((s) => s.input), [{ type: "input", content: "hello", requestId: "event:5", whenBusy: "steer", entry: ENTRY }]);
  assert.equal(r.submitted[0]!.ctx, BACKGROUND_CONTEXT);
});

test("conversationOf submit sends no entry for an input without one", async () => {
  const r = rig();
  await r.conversation.submit({ content: "hello", requestId: "r1", whenBusy: "followUp" } as never);
  const input = r.submitted[0]!.input as Record<string, unknown>;
  assert.deepEqual(input, { type: "input", content: "hello", requestId: "r1", whenBusy: "followUp" });
  assert.equal("entry" in input, false);
});

test("conversationOf submit resolves wait with the status of the submission", async () => {
  for (const status of ["done", "unanswered"] as const) {
    const r = rig(status === "done" ? { status, entry: "e1" } : { status });
    const submission = await r.conversation.submit({ content: "x", requestId: "r", whenBusy: "followUp", entry: ENTRY });
    assert.equal(await submission.wait(), status);
  }
});

test("conversationOf write submits the write unchanged to the root", async () => {
  const r = rig();
  const write = {
    type: "write" as const,
    entry: { kind: "cpd-context", data: { event: 5, delivered_to: "s1" }, model: [{ role: "user", content: "text" }] },
    requestId: "ctx:1",
    text: "context",
  };
  await r.conversation.write(write);
  assert.deepEqual(r.submitted.map((s) => s.input), [write]);
  assert.equal(r.submitted[0]!.ctx, BACKGROUND_CONTEXT);
});

test("conversationOf write rejects when the submission does not end done", async () => {
  for (const status of [{ status: "unanswered" }, { status: "failed" }] as const) {
    const r = rig(status);
    await assert.rejects(
      r.conversation.write({ type: "write", entry: { kind: "cpd-context", data: {}, model: [] }, requestId: "ctx:2" }),
      (e: unknown) => e instanceof Error,
    );
  }
});

test("conversationOf entries returns the active entries of the context", async () => {
  const stored = [{ id: "a", kind: "cpd-event", data: { id: 1 }, model: [] }, { id: "b", kind: "pi.user", data: {}, model: [] }];
  const r = rig(undefined, stored);
  assert.deepEqual(await r.conversation.entries(), stored);
  assert.deepEqual(r.contexts, [BACKGROUND_CONTEXT]);
});

test("conversationOf install installs the extension in the registry", () => {
  const r = rig();
  const extension = { sections: [] };
  r.conversation.install(extension as never);
  assert.deepEqual(r.installed, [extension]);
  assert.equal(r.conversation.closed, r.closed);
});

test("conversationOf submit resolves the id and the status of the submission", async () => {
  const r = rig({ status: "done", entry: "e1" });
  const submission = await r.conversation.submit({ content: "x", requestId: "r", whenBusy: "followUp", entry: ENTRY });
  assert.equal(submission.id, 7);
  assert.equal(await submission.status(), "done");
});

test("conversationOf watch returns the view and delivers each call to the listener", async () => {
  const r = rig();
  const stream = await r.conversation.watch();
  assert.equal(stream.value, r.stream.value);
  assert.deepEqual(r.watched, [[BACKGROUND_CONTEXT]]);
  const got: unknown[] = [];
  stream.start(async (call) => void got.push(call));
  assert.equal(r.started.length, 1);
  const call = { kind: "value", value: { n: 2 }, ops: [["a", ["x"], "y"]], retired: false };
  await r.started[0]!(call, { context: true });
  assert.deepEqual(got, [call], "the context argument is dropped");
});

test("conversationOf watch stop stops the watch", async () => {
  const r = rig();
  const stream = await r.conversation.watch();
  await stream.stop();
  assert.deepEqual(r.stopped, ["stop"]);
});

test("conversationOf watch passes the end of the watch as closed", async () => {
  const r = rig();
  const stream = await r.conversation.watch();
  assert.equal(await stream.closed, r.end);
});

test("conversationOf abort passes the background option to the root", async () => {
  const r = rig();
  await r.conversation.abort({ background: true });
  await r.conversation.abort({ background: false });
  assert.deepEqual(r.aborts, [{ ctx: BACKGROUND_CONTEXT, opts: { background: true } }, { ctx: BACKGROUND_CONTEXT, opts: { background: false } }]);
});

test("conversationOf abortSubmission returns the outcome of the harness", async () => {
  const r = rig();
  assert.equal(await r.conversation.abortSubmission(9), "settled");
  assert.deepEqual(r.abortedIds, [[9, BACKGROUND_CONTEXT, "conv1"]]);
});
