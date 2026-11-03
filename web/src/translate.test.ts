// Host-frame translation tests of protocol 2 (web/README.md "`web/src/translate.ts` — the frame
// translation", docs/contract.md §6.14). RED: `applyHostFrame` throws `not implemented` for view and
// ops frames. The frames are built from the pi-durable 1.5.0 types, not recorded; ops are literals
// (the tests do not import `@earendil-works/chord`). No socket, no cpd, no network.
import { expect, test } from "vitest";
import { applyHostFrame, translateFrames, type HostState } from "./translate";
import type { HostFrame } from "./api/host";

const EPOCH = "e1";

function empty(): HostState {
  return { view: undefined, messages: [], cursor: undefined, lastEntryId: undefined };
}

function userEntry(id: number, text: string) {
  return { id, conversationId: 1, kind: "pi.user", model: [{ role: "user", content: text }] };
}

function assistantEntry(id: number, text: string) {
  return {
    id,
    conversationId: 1,
    kind: "pi.assistant",
    model: [
      {
        role: "assistant",
        content: [
          { type: "text", text },
          { type: "thinking", thinking: "hidden" },
        ],
      },
    ],
  };
}

function viewOf(entries: unknown[], before?: number) {
  const view: Record<string, unknown> = {
    conversation: { id: 1 },
    entries,
    docs: { "pi.agent": {}, "pi.live": {}, "pi.inbox": {}, "pi.provider": {}, "pi.usage": {} },
  };
  if (before !== undefined) view["before"] = before;
  return view;
}

function viewFrame(seq: number, view: unknown): HostFrame {
  return { type: "view", epoch: EPOCH, seq, view };
}

function opsFrame(seq: number, ops: unknown[]): HostFrame {
  return { type: "ops", epoch: EPOCH, seq, ops };
}

const texts = (state: HostState): string[] => state.messages.map((message) => message.text);

test("translateFrames turns a view frame into one message per entry", () => {
  const state = translateFrames(empty(), [
    viewFrame(1, viewOf([userEntry(1, "run the checks"), assistantEntry(2, "ok, done")])),
  ]);
  expect(state.messages).toEqual([
    { id: "host-1", role: "user", text: "run the checks" },
    { id: "host-2", role: "assistant", text: "ok, done" },
  ]);
  expect(state.cursor).toEqual({ epoch: EPOCH, seq: 1 });
  expect(state.lastEntryId).toBe(2);
});

test("translateFrames adds the entry an ops frame appends", () => {
  const base = translateFrames(empty(), [viewFrame(1, viewOf([userEntry(1, "hi")]))]);
  const next = applyHostFrame(base, opsFrame(2, [["p", ["entries"], 1, 0, [assistantEntry(2, "hello")]]]));
  expect(texts(next)).toEqual(["hi", "hello"]);
  expect(next.lastEntryId).toBe(2);
  // Pure: the input state is untouched.
  expect(texts(base)).toEqual(["hi"]);
});

test("translateFrames adds no message for an ops frame that changes only docs", () => {
  const base = translateFrames(empty(), [viewFrame(1, viewOf([userEntry(1, "hi")]))]);
  const live = { run: { id: "r1" }, generation: { message: { role: "assistant", content: "par" } } };
  const one = applyHostFrame(base, opsFrame(2, [["s", ["docs", "pi.live"], live]]));
  expect(texts(one)).toEqual(["hi"]);
  expect(one.cursor).toEqual({ epoch: EPOCH, seq: 2 });
  const two = applyHostFrame(one, opsFrame(3, [["d", ["docs", "pi.live"]]]));
  expect(texts(two)).toEqual(["hi"]);
  expect(two.cursor).toEqual({ epoch: EPOCH, seq: 3 });
});

test("translateFrames adds only the entries after the last id on a root replacement", () => {
  const base = translateFrames(empty(), [
    viewFrame(1, viewOf([userEntry(1, "one"), assistantEntry(2, "two")])),
  ]);
  const replaced = viewOf([userEntry(1, "one"), assistantEntry(2, "two"), userEntry(3, "three")]);
  const next = applyHostFrame(base, opsFrame(2, [["r", replaced]]));
  expect(texts(next)).toEqual(["one", "two", "three"]);
  expect(next.lastEntryId).toBe(3);
});

test("translateFrames keeps the messages of entries a window drops", () => {
  const base = translateFrames(empty(), [
    viewFrame(1, viewOf([userEntry(1, "one"), assistantEntry(2, "two")])),
  ]);
  const next = applyHostFrame(
    base,
    opsFrame(2, [
      ["p", ["entries"], 2, 0, [userEntry(3, "three")]],
      ["p", ["entries"], 0, 1, []],
      ["s", ["before"], 1],
    ]),
  );
  expect(texts(next)).toEqual(["one", "two", "three"]);
  expect((next.view as { entries: unknown[] }).entries).toHaveLength(2);
});

test("translateFrames refuses an ops frame before any view", () => {
  expect(() => applyHostFrame(empty(), opsFrame(1, [["s", ["docs", "pi.live"], {}]]))).toThrow();
  // The same ops frame after a view does not throw.
  const base = applyHostFrame(empty(), viewFrame(1, viewOf([])));
  expect(() => applyHostFrame(base, opsFrame(2, [["s", ["docs", "pi.live"], {}]]))).not.toThrow();
});

test("translateFrames advances the cursor with each frame", () => {
  const frames: HostFrame[] = [
    viewFrame(1, viewOf([])),
    opsFrame(2, [["p", ["entries"], 0, 0, [userEntry(1, "a")]]]),
    opsFrame(3, [["s", ["docs", "pi.agent"], { cwd: "/x" }]]),
  ];
  let state = empty();
  const cursors = [];
  for (const frame of frames) {
    state = applyHostFrame(state, frame);
    cursors.push(state.cursor);
  }
  expect(cursors).toEqual([
    { epoch: EPOCH, seq: 1 },
    { epoch: EPOCH, seq: 2 },
    { epoch: EPOCH, seq: 3 },
  ]);
  // Non-stream frames leave the state unchanged.
  expect(applyHostFrame(state, { type: "host_state", state: "idle" })).toEqual(state);
});
