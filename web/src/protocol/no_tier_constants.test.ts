import { describe, it, expect } from "vitest";
import * as tierModule from "./tier";

describe("no tier grammar constants or local ladder math remain in web", () => {
  it("no tier grammar constants or local ladder math remain in web", () => {
    const forbidden = [
      "MODEL_LETTERS",
      "EFFORTS",
      "parseTier",
      "tierIndex",
      "tierAt",
      "TIER_COUNT",
    ];
    for (const name of forbidden) {
      expect((tierModule as Record<string, unknown>)[name], `Expected ${name} to not be exported`).toBeUndefined();
    }
  });
});
