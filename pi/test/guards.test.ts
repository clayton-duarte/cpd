import assert from "node:assert/strict";
import { test } from "node:test";

// docs/architecture/pi-native-core.md §4.6 & §7 (Plan 3: skills-to-tools).
// Pins:
// - Guards migrate from text regex scanning of bash tool calls to typed tool-call hooks inside pi
// - testguard: red test file immutability enforced directly by tool guard
// - mainguard: primary checkout immutability enforced directly by tool guard
// - Tool guards reject illegal modifications before tool execution

test("tool guard enforces test and main immutability", async () => {
  // Tool hooks must be registered and active on pi tool_call.
  // When active:
  // 1. Attempts to edit or write to red test files by implementation Jobs are refused by testguard tool hook.
  // 2. Attempts to modify primary checkout are refused by mainguard tool hook.
  // 3. Guards inspect structured tool arguments, not bash regexes.

  // In the current branch before migration, tool guard hooks for test immutability
  // and typed tool call guards are not yet implemented/active.
  const toolGuardActive = false; // Stub: will be true when tool hooks are active

  assert.ok(
    toolGuardActive,
    "Typed tool guards for test and main checkout immutability must be active",
  );
});
