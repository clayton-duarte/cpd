import assert from "node:assert/strict";
import { test } from "node:test";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { conversationOf } from "../../durable-worker/harness.ts";

type Status = { status: "done"; entry: string } | { status: "unanswered" } | { status: "failed" };

/** A fake root: `submit` records its argument and returns a fake submission; no pi-durable store, no model, no timer. */
function rig(status: Status = { status: "done", entry: "e1" }, entries: readonly unknown[] = []) {
  const submitted: { input: unknown; ctx: unknown }[] = [];
  const root = {
    submit: async (input: unknown, ctx: unknown) => {
      submitted.push({ input, ctx });
      return {
        wait: async () => ({ status: status.status === "done" ? "done" : status.status }),
        status: async () => status,
      };
    },
    viewState: async () => ({ value: { entries } }),
  };
  const installed: unknown[] = [];
  const registry = { install: (extension: unknown) => void installed.push(extension) };
  const closed = new Promise<unknown>(() => {});
  return { submitted, installed, closed, conversation: conversationOf(root, registry, closed) };
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

test("conversationOf entries returns the entries of the view state", async () => {
  const stored = [{ id: "a", kind: "cpd-event", data: { id: 1 }, model: [] }, { id: "b", kind: "pi.user", data: {}, model: [] }];
  const r = rig(undefined, stored);
  assert.deepEqual(await r.conversation.entries(), stored);
});

test("conversationOf install installs the extension in the registry", () => {
  const r = rig();
  const extension = { sections: [] };
  r.conversation.install(extension as never);
  assert.deepEqual(r.installed, [extension]);
  assert.equal(r.conversation.closed, r.closed);
});
