import { describe, expect, it } from "vitest";
import { parseCockpitAction } from "./cockpit.ts";

describe("parseCockpitAction", () => {
  it("parseCockpitAction rejects an unknown kind with unknown_action", () => {
    expect(() => parseCockpitAction({ kind: "inbox.explode", itemId: 1 })).toThrowError(expect.objectContaining({ code: "unknown_action" }));
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
