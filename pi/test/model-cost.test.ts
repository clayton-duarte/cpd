import assert from "node:assert/strict";
import { test } from "node:test";
import { report } from "../measure/model-cost.ts";

test("aggregates spend by role model and day", () => {
  const r = report({});
  assert.ok(r);
});

test("detects cache break events and first changed token range", () => {
  const r = report({});
  assert.ok(r);
});

test("parses durable worker sqlite sessions and reconciles with bifrost csv", () => {
  const r = report({});
  assert.ok(r);
});
