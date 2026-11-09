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

  it("J is a valid tier letter", () => {
    // The decide letter J (contract §9, air-d113, air-d115, air-d120): J3 and JX are Tiers,
    // and an effort J's model does not offer (J5) is still refused.
    const j3 = { type: "settings.save", changes: { tiers: { worker: "J3" } } };
    expect(parseClientMessage(j3)).toEqual(j3);
    const jx = { type: "settings.save", changes: { tiers: { auxiliar: "JX" } } };
    expect(parseClientMessage(jx)).toEqual(jx);
    expect(() => parseClientMessage({ type: "settings.save", changes: { tiers: { worker: "J5" } } })).toThrowError(
      expect.objectContaining({ code: "invalid_message" }),
    );
  });
});

describe("settings.save changes", () => {
  const save = (changes: unknown) => ({ type: "settings.save", changes });
  it("accepts a role's model and effort as separate fields, catalog letters and efforts only", () => {
    for (const model of ["H", "S", "O", "G", "M", "K", "L", "P", "A", "J"]) {
      for (const effort of ["0", "1", "X", "2"]) {
        const raw = save({ tiers: { worker: { model, effort } } });
        expect(parseClientMessage(raw)).toEqual(raw);
      }
    }
    for (const bad of [{ model: "Z", effort: "1" }, { model: "H", effort: "3" }, { model: "H", effort: "5" }, { model: "h", effort: "1" }, { model: "H" }, { effort: "1" }]) {
      expect(() => parseClientMessage(save({ tiers: { worker: bad } }))).toThrowError(expect.objectContaining({ code: "invalid_message" }));
    }
  });

  it("rejects a combined Tier string once model and effort are separate", () => {
    expect(() => parseClientMessage(save({ tiers: { worker: "H1" } }))).toThrowError(expect.objectContaining({ code: "invalid_message" }));
  });

  it("parseChanges keeps a concurrency-only change", () => {
    const raw = save({ concurrency: 12 });
    expect(parseClientMessage(raw)).toEqual(raw);
  });

  it("parseChanges rejects a non-integer concurrency", () => {
    for (const concurrency of [1.5, "12", Number.NaN, null]) {
      expect(() => parseClientMessage(save({ concurrency }))).toThrowError(expect.objectContaining({ code: "invalid_message" }));
    }
  });

  it("parseChanges accepts decide in tiers", () => {
    const raw = save({ tiers: { decide: { model: "J", effort: "X" } } });
    expect(parseClientMessage(raw)).toEqual(raw);
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

  it("session.completion keeps the others scores", () => {
    const others = [{ label: "open_work", score: 30 }];
    const raw = { type: "session.completion", sessionId: "s1", completion: { ...completion, others } };
    expect(parseServerMessage(raw)).toEqual(raw);
    const msg = parseServerMessage({ type: "session.completion", sessionId: "s1", completion }) as { completion: object };
    expect("others" in msg.completion).toBe(false);
  });

  it("session.completion refuses malformed others", () => {
    const bads: unknown[] = ["x", [{ label: "bogus", score: 1 }], [{ label: "open_work", score: "1" }], [{ label: "open_work", score: Number.POSITIVE_INFINITY }], [{ label: "open_work", score: Number.NaN }]];
    for (const others of bads) {
      const raw = { type: "session.completion", sessionId: "s1", completion: { ...completion, others } };
      expect(() => parseServerMessage(raw)).toThrowError(expect.objectContaining({ code: "protocol_error", details: expect.objectContaining({ field: "others" }) }));
    }
  });
});
