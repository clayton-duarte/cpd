// Chat-fold tests (docs/research/web-client-api.md §5, hs-d772). RED: `fold` still throws
// `not implemented`, so they fail now and pass once the impl lands.
import { expect, test } from "vitest";
import { fold, type ChatView } from "./fold";

test("the fold applies an events batch to the snapshot state", () => {
  const view: ChatView = { epoch: "e1", seq: 5, messages: [], cursor: { epoch: "e1", seq: 5 } };
  const next = fold(view, {
    type: "events",
    epoch: "e1",
    seq: 6,
    events: [{ kind: "message_start", id: "m1", role: "assistant", text: "hel" }],
  });
  // each AgentEvent of the batch is applied in order and the cursor advances.
  expect(next.seq).toBe(6);
  expect(next.cursor).toEqual({ epoch: "e1", seq: 6 });
  expect(next.messages.find((m) => m.id === "m1")?.text).toBe("hel");
});

test("message_start replaces the whole message at message_end", () => {
  const view: ChatView = { epoch: "e1", seq: 0, messages: [], cursor: { epoch: "e1", seq: 0 } };
  const started = fold(view, {
    type: "events",
    epoch: "e1",
    seq: 1,
    events: [{ kind: "message_start", id: "m1", role: "assistant", text: "hel" }],
  });
  expect(started.messages.find((m) => m.id === "m1")?.text).toBe("hel");
  const ended = fold(started, {
    type: "events",
    epoch: "e1",
    seq: 2,
    events: [{ kind: "message_end", id: "m1", text: "hello world" }],
  });
  // the whole message is replaced at message_end, not concatenated.
  expect(ended.messages.find((m) => m.id === "m1")?.text).toBe("hello world");
});

test("a snapshot frame replaces the whole view and a cursor resumes", () => {
  const view: ChatView = {
    epoch: "e0",
    seq: 99,
    messages: [{ id: "old", role: "assistant", text: "stale", pending: false }],
    cursor: { epoch: "e0", seq: 99 },
  };
  const next = fold(view, { type: "snapshot", epoch: "e1", seq: 0, snapshot: { messages: [] } });
  // the snapshot replaces the whole view (no stale message survives).
  expect(next.epoch).toBe("e1");
  expect(next.seq).toBe(0);
  expect(next.messages.find((m) => m.id === "old")).toBeUndefined();
  // the cursor resumes to the frame's epoch/seq.
  expect(next.cursor).toEqual({ epoch: "e1", seq: 0 });
});
