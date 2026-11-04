import { describe, expect, it } from "vitest";
import { parseClientMessage } from "./wire.ts";

describe("parseClientMessage", () => {
  it("parseClientMessage accepts a well-formed session submit", () => {
    const raw = { type: "session.submit", sessionId: "s1", clientSubmissionId: "c1", text: "hi", images: [], whenBusy: "followUp" };
    expect(parseClientMessage(raw)).toEqual(raw);
  });

  it("parseClientMessage rejects an unknown message type with invalid_message", () => {
    expect(() => parseClientMessage({ type: "bogus" })).toThrowError(expect.objectContaining({ code: "invalid_message" }));
  });
});
