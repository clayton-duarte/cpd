import { describe, expect, it } from "vitest";
import { parseCockpitAction } from "./cockpit.ts";

describe("parseCockpitAction", () => {
  it("parseCockpitAction rejects an unknown kind with unknown_action", () => {
    expect(() => parseCockpitAction({ kind: "inbox.explode", itemId: 1 })).toThrowError(expect.objectContaining({ code: "unknown_action" }));
  });
});
