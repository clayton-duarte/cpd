import { describe, expect, it } from "vitest";

describe("fails when shell code changes without version bump", () => {
  it("fails when shell code changes without version bump", () => {
    // Red test: check script fails if shell code changed without bumping shell version.
    expect(true).toBe(false);
  });
});
