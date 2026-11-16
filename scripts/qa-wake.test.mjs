import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

test("scheduled QA wake uses the same session and refuses duplicate or unsafe delivery", () => {
  const result = spawnSync("python3", [fileURLToPath(new URL("./qa-wake.test.py", import.meta.url))], {
    encoding: "utf8", timeout: 20_000, env: { ...process.env, PYTHONDONTWRITEBYTECODE: "1" },
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
});
