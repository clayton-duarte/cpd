import assert from "node:assert/strict";
import test from "node:test";
import { calculateModelInputCap, enforceModelInputCap } from "../lib/model-input-cap.ts";

test("cap calculation matches pro-d559 for 1M, 256K, 200K, 128K windows", () => {
  assert.equal(calculateModelInputCap(1_000_000), 200_000);
});

test("boundary: input equal to cap is sent, cap + 1 compacts", () => {
  assert.fail("stub not implemented");
});

test("prompt and tool-schema overhead is counted in effective input", () => {
  assert.fail("stub not implemented");
});

test("oversized incoming message or tool result is handled", () => {
  assert.fail("stub not implemented");
});

test("model downswitch recalculates cap before next request", () => {
  assert.fail("stub not implemented");
});

test("oversized input after compaction refuses dispatch with explicit error", () => {
  assert.fail("stub not implemented");
});

test("stored transcript is preserved in full", () => {
  assert.fail("stub not implemented");
});

test("invalid or missing context window metadata fails loud", () => {
  assert.fail("stub not implemented");
});
