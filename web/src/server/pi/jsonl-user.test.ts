import { expect, it } from "vitest";
import { normalizeText } from "./jsonl.ts";

const line = (o: unknown): string => JSON.stringify(o);

it("a session-message sent by the web chat is a user message even when from is a session id", () => {
  const text = line({
    type: "custom_message",
    id: "w1",
    customType: "cpd",
    content: "Message from cpd-context (75b61f97) (16:03 EST): hello from the web",
    display: true,
    timestamp: "2026-11-03T20:28:42.748Z",
    details: {
      id: 1,
      at: "2026-11-03T20:28:42.459Z",
      kind: "session-message",
      text: "Message from cpd-context: hello from the web",
      data: { from: "75b61f97-8377-4feb-97e6-2f1c1d1ed439", text: "hello from the web", wake: true, unblocks: "User message from the web chat" },
    },
  });
  const { items } = normalizeText(text);
  expect(items).toHaveLength(1);
  expect(items[0]).toMatchObject({ kind: "user", text: "hello from the web" });
});
