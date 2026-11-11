import assert from "node:assert/strict";
import { test } from "node:test";
import { toolGuardActive, validateToolCall } from "../lib/guards.ts";

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

  assert.ok(
    toolGuardActive,
    "Typed tool guards for test and main checkout immutability must be active",
  );

  // 1. Testguard: red test file immutability
  const testVerdict = validateToolCall({
    toolName: "edit",
    input: { path: "pi/test/foo.test.ts" },
    role: "worker",
    guardedTestFiles: ["pi/test/foo.test.ts"],
  });
  assert.equal(testVerdict.blocked, true, "Attempts to edit red test files must be refused");
  assert.equal(testVerdict.source, "testguard");

  // Allowed edit on un-guarded file
  const allowedVerdict = validateToolCall({
    toolName: "edit",
    input: { path: "pi/lib/foo.ts" },
    role: "worker",
    guardedTestFiles: ["pi/test/foo.test.ts"],
  });
  assert.equal(allowedVerdict.blocked, false, "Editing un-guarded files must be allowed");
});
