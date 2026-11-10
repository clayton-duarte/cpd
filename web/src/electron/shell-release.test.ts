import { describe, expect, it } from "vitest";
import { SHELL_VERSION, checkShellVersionBump } from "./version.ts";

describe("fails when shell code changes without version bump", () => {
  it("fails when shell code changes without version bump", () => {
    // Contract (docs/implementing.md "The shell ABI check", commands.md §14.4b):
    // A check fails if shell code changed without bumping shell version.
    // The shell version names the release (app-v<shell version>).
    // A shell change cannot ship under an old version.
    expect(SHELL_VERSION).toBeDefined();

    const shellChangedFiles = ["web/src/electron/main.ts", "web/src/electron/watchdog.ts"];
    const versionBumped = false;

    const result = checkShellVersionBump(shellChangedFiles, versionBumped);
    expect(result.ok).toBe(false);
    expect(result.error).toContain("shell code changed without version bump");
  });
});

