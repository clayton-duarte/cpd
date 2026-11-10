import { describe, it, expect } from "vitest";
import { parseAgentKey, isValidAgentKey } from "./agentKey.ts";

describe("agentKey", () => {
  it("parses and validates manager lead and worker agent keys", () => {
    // Manager: bare context id
    expect(parseAgentKey("cpd")).toEqual({
      kind: "manager",
      context: "cpd",
    });
    expect(isValidAgentKey("cpd")).toBe(true);

    // Manager with hyphen and numbers (min 2 chars, starts with letter)
    expect(parseAgentKey("c1-abc")).toEqual({
      kind: "manager",
      context: "c1-abc",
    });
    expect(isValidAgentKey("c1-abc")).toBe(true);

    // Lead: context:plan
    expect(parseAgentKey("cpd:plan-x")).toEqual({
      kind: "lead",
      context: "cpd",
      plan: "plan-x",
    });
    expect(isValidAgentKey("cpd:plan-x")).toBe(true);

    // Worker: context:plan:job
    expect(parseAgentKey("cpd:plan-x:web-tests")).toEqual({
      kind: "worker",
      context: "cpd",
      plan: "plan-x",
      job: "web-tests",
    });
    expect(isValidAgentKey("cpd:plan-x:web-tests")).toBe(true);

    // Shortest worker key: context (2) + ':' + plan (2) + ':' + job (1) = 7 chars
    expect(parseAgentKey("ab:cd:e")).toEqual({
      kind: "worker",
      context: "ab",
      plan: "cd",
      job: "e",
    });
    expect(isValidAgentKey("ab:cd:e")).toBe(true);
  });

  it("rejects invalid agent keys and lengths", () => {
    // Empty
    expect(parseAgentKey("")).toBeNull();
    expect(isValidAgentKey("")).toBe(false);

    // Uppercase is invalid
    expect(parseAgentKey("CPD")).toBeNull();
    expect(isValidAgentKey("CPD")).toBe(false);
    expect(parseAgentKey("cpd:Plan-x")).toBeNull();
    expect(isValidAgentKey("cpd:Plan-x")).toBe(false);

    // Context single character (must be 2-40: ^[a-z][a-z0-9-]{1,39}$)
    expect(parseAgentKey("c")).toBeNull();
    expect(isValidAgentKey("c")).toBe(false);

    // Context starting with digit or hyphen
    expect(parseAgentKey("1cpd")).toBeNull();
    expect(isValidAgentKey("1cpd")).toBe(false);
    expect(parseAgentKey("-cpd")).toBeNull();
    expect(isValidAgentKey("-cpd")).toBe(false);

    // Plan single character (must be 2-24: ^[a-z][a-z0-9-]{1,23}$)
    expect(parseAgentKey("cpd:p")).toBeNull();
    expect(isValidAgentKey("cpd:p")).toBe(false);

    // Too many parts (more than 2 colons)
    expect(parseAgentKey("cpd:plan-x:job-y:extra")).toBeNull();
    expect(isValidAgentKey("cpd:plan-x:job-y:extra")).toBe(false);

    // Trailing or leading colons
    expect(parseAgentKey(":cpd")).toBeNull();
    expect(parseAgentKey("cpd:")).toBeNull();
    expect(parseAgentKey("cpd::job")).toBeNull();

    // Context too long (> 40 chars)
    const tooLongContext = "a" + "b".repeat(40);
    expect(parseAgentKey(tooLongContext)).toBeNull();
    expect(isValidAgentKey(tooLongContext)).toBe(false);

    // Total length check: max 98 characters
    // 40 context + 1 colon + 24 plan + 1 colon + 32 job = 98 max
    const maxContext = "c" + "x".repeat(39); // 40
    const maxPlan = "p" + "x".repeat(23); // 24
    const maxJob = "j" + "x".repeat(31); // 32
    const exactMaxKey = `${maxContext}:${maxPlan}:${maxJob}`;
    expect(exactMaxKey.length).toBe(98);
    // Over max length (> 98 chars)
    const overMaxKey = exactMaxKey + "z";
    expect(parseAgentKey(overMaxKey)).toBeNull();
    expect(isValidAgentKey(overMaxKey)).toBe(false);
  });
});
