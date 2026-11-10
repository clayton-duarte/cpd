import { describe, expect, it } from "vitest";
import { MIN_SHELL_ABI_VERSION, SHELL_ABI_VERSION, SHELL_VERSION } from "./version.ts";

describe("payload refuses incompatible shell version", () => {
  it("payload refuses incompatible shell version", () => {
    // Contract (docs/implementing.md "The shell ABI check"):
    // The shell declares its ABI version (SHELL_ABI_VERSION).
    // Each payload declares the minimum shell ABI it needs (MIN_SHELL_ABI_VERSION).
    // The shell loads a payload only when its own ABI is at least that minimum.
    // Otherwise it refuses the payload, keeps the current one, and reports an error Event
    // with the message 'new CPD.app release needed'.

    // Stub verification: ensure exports exist
    expect(SHELL_VERSION).toBeDefined();
    expect(SHELL_ABI_VERSION).toBeDefined();
    expect(MIN_SHELL_ABI_VERSION).toBeDefined();

    // Verify ABI compatibility logic:
    // When running shell ABI is lower than payload required ABI, load must be refused
    // and 'new CPD.app release needed' reported.
    // In stub state, checkAbi function or compatibility gate is not implemented yet.
    type CheckAbi = (shellAbi: number, payloadMinAbi: number) => { compatible: boolean; error?: string };
    const checkAbi: CheckAbi = (_shell, _payloadMin) => {
      // Stub returns compatible: true without error
      return { compatible: true };
    };

    const result = checkAbi(1, 2);
    expect(result.compatible).toBe(false);
    expect(result.error).toBe("new CPD.app release needed");
  });
});
