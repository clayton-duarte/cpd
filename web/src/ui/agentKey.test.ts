import { describe, it, expect } from "vitest";
import { parseAgentKey, isValidAgentKey } from "./agentKey.ts";

describe("agentKey", () => {
  it("parses and validates manager lead and worker agent keys", () => {
    expect(parseAgentKey("cpd")).toEqual({
      kind: "manager",
      context: "cpd",
    });
    expect(isValidAgentKey("cpd")).toBe(true);

    expect(parseAgentKey("cpd:plan-x")).toEqual({
      kind: "lead",
      context: "cpd",
      plan: "plan-x",
    });
    expect(isValidAgentKey("cpd:plan-x")).toBe(true);

    expect(parseAgentKey("cpd:plan-x:web-tests")).toEqual({
      kind: "worker",
      context: "cpd",
      plan: "plan-x",
      job: "web-tests",
    });
    expect(isValidAgentKey("cpd:plan-x:web-tests")).toBe(true);
  });

  it("rejects invalid agent keys and lengths", () => {
    expect(parseAgentKey("")).toBeNull();
    expect(isValidAgentKey("")).toBe(false);

    expect(parseAgentKey("CPD")).toBeNull();
    expect(isValidAgentKey("CPD")).toBe(false);

    expect(parseAgentKey("cpd:plan-x:job-y:extra")).toBeNull();
    expect(isValidAgentKey("cpd:plan-x:job-y:extra")).toBe(false);

    const tooLongContext = "a" + "b".repeat(40);
    expect(parseAgentKey(tooLongContext)).toBeNull();
    expect(isValidAgentKey(tooLongContext)).toBe(false);
  });
});
