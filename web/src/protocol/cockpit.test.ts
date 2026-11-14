import { describe, expect, it } from "vitest";
import { parseCockpitAction, parseInboxEngine } from "./cockpit.ts";

describe("parseCockpitAction", () => {
  it("parseCockpitAction rejects an unknown kind with unknown_action", () => {
    expect(() => parseCockpitAction({ kind: "inbox.explode", itemId: 1 })).toThrowError(expect.objectContaining({ code: "unknown_action" }));
  });

  it("parses context.new with id, title and owner", () => {
    const a = { kind: "context.new", contextId: "ctx-1", title: "My context", owner: "clayton-duarte" };
    expect(parseCockpitAction(a)).toEqual(a);
    // all three fields are required non-empty strings (CONTRACT 20.1)
    for (const bad of [
      { kind: "context.new", title: "My context", owner: "clayton-duarte" },
      { kind: "context.new", contextId: "ctx-1", owner: "clayton-duarte" },
      { kind: "context.new", contextId: "ctx-1", title: "My context" },
      { kind: "context.new", contextId: "", title: "My context", owner: "clayton-duarte" },
      { kind: "context.new", contextId: "ctx-1", title: "My context", owner: "" },
    ]) {
      expect(() => parseCockpitAction(bad), JSON.stringify(bad)).toThrowError(expect.objectContaining({ code: "invalid_message" }));
    }
  });

  it("pr.merge parses without fields and pr.post and pr.repost need a plan", () => {
    expect(parseCockpitAction({ kind: "pr.merge" })).toEqual({ kind: "pr.merge" });
    expect(parseCockpitAction({ kind: "pr.post", planId: "p1" })).toEqual({ kind: "pr.post", planId: "p1" });
    expect(parseCockpitAction({ kind: "pr.repost", planId: "p1", pull: "a/b#1" })).toEqual({ kind: "pr.repost", planId: "p1", pull: "a/b#1" });
    expect(() => parseCockpitAction({ kind: "pr.post" })).toThrowError(expect.objectContaining({ code: "invalid_message" }));
    expect(() => parseCockpitAction({ kind: "pr.repost" })).toThrowError(expect.objectContaining({ code: "invalid_message" }));
  });
  it("parseCockpitAction accepts error.report with target cpd", () => {
    const a = { kind: "error.report", target: "cpd", code: "protocol_error", message: "bad" };
    expect(parseCockpitAction(a)).toEqual(a);
  });
});

describe("parseInboxEngine", () => {
  it("parses engineDownSince and engineRestartReason", () => {
    expect(parseInboxEngine({ engineRunning: false, engineDownSince: "2026-01-15T12:00:00Z", engineRestartReason: "install" })).toEqual({
      engineRunning: false, engineDownSince: "2026-01-15T12:00:00Z", engineRestartReason: "install",
    });
    expect(parseInboxEngine({ engineRunning: true, engineDownSince: null, engineRestartReason: null })).toEqual({
      engineRunning: true, engineDownSince: null, engineRestartReason: null,
    });
    expect(() => parseInboxEngine({ engineRunning: false, engineDownSince: 5 })).toThrowError(expect.objectContaining({ code: "invalid_message" }));
  });
});

it("session.resume parses and rejects malformed actions", () => {
  expect(parseCockpitAction({ kind: "session.resume", sessionId: "s1" })).toEqual({ kind: "session.resume", sessionId: "s1" });
  for (const action of [{ kind: "session.resume" }, { kind: "session.resume", sessionId: "" }, { kind: "session.resume", sessionId: 3 }]) {
    expect(() => parseCockpitAction(action)).toThrowError(expect.objectContaining({ code: "invalid_message" }));
  }
});
