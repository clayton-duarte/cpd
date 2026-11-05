import { describe, expect, it } from "vitest";
import { parseClientMessage, parseServerMessage } from "./wire.ts";

describe("parseClientMessage", () => {
  it("parseClientMessage accepts a well-formed session submit", () => {
    const raw = { type: "session.submit", sessionId: "s1", clientSubmissionId: "c1", text: "hi", images: [], whenBusy: "followUp" };
    expect(parseClientMessage(raw)).toEqual(raw);
  });

  it("parseClientMessage rejects an unknown message type with invalid_message", () => {
    expect(() => parseClientMessage({ type: "bogus" })).toThrowError(expect.objectContaining({ code: "invalid_message" }));
  });
});

describe("parseServerMessage", () => {
  const completion = { label: "safe_to_archive", text: "Safe to archive · 93%", score: 93, tone: "done" };
  it("session.completion parses with a completion", () => {
    const raw = { type: "session.completion", sessionId: "s1", completion };
    expect(parseServerMessage(raw)).toEqual(raw);
  });

  it("session.completion parses with a null completion", () => {
    const raw = { type: "session.completion", sessionId: "s1", completion: null };
    expect(parseServerMessage(raw)).toEqual(raw);
  });

  it("session.completion rejects a score that is not a number", () => {
    const raw = { type: "session.completion", sessionId: "s1", completion: { ...completion, score: "93" } };
    expect(() => parseServerMessage(raw)).toThrowError(expect.objectContaining({ code: "protocol_error" }));
  });
});
