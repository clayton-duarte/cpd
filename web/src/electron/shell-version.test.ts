import { describe, expect, it } from "vitest";

describe("payload refuses incompatible shell version", () => {
  it("payload refuses incompatible shell version", () => {
    // Red test: payload declares required shell version/ABI; incompatible shell version is refused
    // and reported with error event 'new CPD.app release needed'.
    expect(true).toBe(false);
  });
});
