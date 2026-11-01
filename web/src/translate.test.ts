// Host-frame translation tests of the dev live adapter (web/README.md "`web/src/translate.ts` — the
// frame translation", hs-d819/chat-host). RED: `applyHostFrame` / `translateFrames` still throw
// `not implemented`, so these fail now and pass once the impl lands.
//
// The frames are the real captured fixture web/src/api/host.fixture.ndjson (read from disk) plus
// inline events of the pi-durable shapes the CLI's Rust tail folds (core/src/session/tail.rs): no
// socket, no cpd, no network.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { applyHostFrame, translateFrames, type HostState } from "./translate";
import type { HostFrame } from "./api/host";

const EPOCH = "157af596-ac7c-41f0-905a-faeb87c5055f";

/** The real captured host frames, read from disk. */
function fixtureFrames(): HostFrame[] {
  return readFileSync(new URL("./api/host.fixture.ndjson", import.meta.url), "utf8")
    .split("\n")
    .filter((line) => line.length > 0)
    .map((line) => JSON.parse(line) as HostFrame);
}

/** A fresh empty fold state. */
function empty(): HostState {
  return { messages: [], cursor: undefined };
}

/** An assistant message of the pi-durable shape, with one text block and one thinking block. */
function assistantMessage(text: string) {
  return {
    role: "assistant",
    content: [
      { type: "text", text },
      { type: "thinking", thinking: "hidden" },
    ],
  };
}

test("translateFrames turns a real host snapshot and events into messages", () => {
  // The captured frames: welcome, attached, an empty snapshot and one bookkeeping event. They fold
  // to no messages but advance the cursor to the last stream frame (doc contract 6.20, rule 4).
  const real = translateFrames(empty(), fixtureFrames());
  expect(real.messages).toEqual([]);
  expect(real.cursor).toEqual({ epoch: EPOCH, seq: 2 });

  // A snapshot replaces the whole state (rule 1): its model messages render like the tail's lines
  // (`host-N` ids in output order), thinking blocks skipped and bookkeeping entries silent.
  const snapshot: HostFrame = {
    type: "snapshot",
    epoch: "e1",
    seq: 1,
    snapshot: {
      type: "snapshot",
      entries: [
        {
          id: 1,
          conversationId: 1,
          kind: "pi.user",
          model: [{ role: "user", content: "run the checks" }],
        },
        {
          id: 2,
          conversationId: 1,
          kind: "pi.assistant",
          model: [assistantMessage("ok, done")],
        },
        {
          id: 3,
          conversationId: 1,
          kind: "cpd-skills",
          model: [],
        },
        {
          id: 4,
          conversationId: 1,
          kind: "pi.tool",
          model: [
            {
              role: "toolResult",
              toolName: "bash",
              isError: true,
              content: [{ type: "text", text: "ENOENT" }],
            },
          ],
        },
      ],
    },
  };
  const folded = translateFrames(empty(), [snapshot]);
  expect(folded.messages).toEqual([
    { id: "host-1", role: "user", text: "run the checks" },
    { id: "host-2", role: "assistant", text: "ok, done" },
    { id: "host-3", role: "tool", name: "bash", text: "ENOENT", error: true },
  ]);
  expect(folded.cursor).toEqual({ epoch: "e1", seq: 1 });

  // An events frame applies its AgentEvents in order (rule 2): `message_end` appends the completed
  // message; a `run_end` completes an in-flight message; other events change nothing shown.
  const events: HostFrame = {
    type: "events",
    epoch: "e1",
    seq: 2,
    events: [
      { type: "message_start", message: assistantMessage("") },
      {
        type: "message_end",
        entry: {
          id: 5,
          conversationId: 1,
          kind: "pi.assistant",
          model: [assistantMessage("second")],
        },
      },
      { type: "submission", record: { id: 6 } },
    ],
  };
  const applied = translateFrames(folded, [events]);
  expect(applied.messages.map((message) => message.text)).toEqual([
    "run the checks",
    "ok, done",
    "ENOENT",
    "second",
  ]);
  expect(applied.cursor).toEqual({ epoch: "e1", seq: 2 });

  // The single-frame primitive is the same fold: `welcome` and `attached` touch nothing.
  const afterAttached = applyHostFrame(folded, {
    type: "attached",
    req: "r1",
    mode: "snapshot",
    epoch: "e1",
    from_seq: 1,
    head_seq: 2,
  });
  expect(afterAttached).toEqual(folded);
});
