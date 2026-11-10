// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { linkifyMentions } from "./mentions.tsx";

describe("chat mentions", () => {
  it("renders agent keys in user assistant event and tool messages as router links", () => {
    const liveKeys = new Set(["cpd", "cpd:plan-x", "cpd:plan-x:web-tests"]);
    const text = "Ask cpd:plan-x for details and worker cpd:plan-x:web-tests for status.";
    const result = linkifyMentions(text, liveKeys);
    expect(result).not.toBeNull();
  });

  it("leaves non-agent keys and invalid syntax as plain text", () => {
    const liveKeys = new Set(["cpd", "cpd:plan-x"]);
    const text = "Mentioning non-agent cpd:plan-y or invalid CPD:plan-x or path /cpd:plan-x";
    const result = linkifyMentions(text, liveKeys);
    expect(result).toBe(text);
  });
});
