import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

// hs-d355 (docs/implementing.md "Running cargo, the Gate, installing"): the Gate is the `verify` script of package.json, in this order, each step stopping the run at its first failure.
test("the verify script runs the portability check and pi-check, in the documented order", () => {
  const pkg = JSON.parse(readFileSync(join(fileURLToPath(import.meta.url), "..", "..", "..", "package.json"), "utf8")) as { scripts: Record<string, string> };
  assert.deepEqual(pkg.scripts.verify.split("&&").map((step) => step.trim()), [
    "scripts/check-migrations-frozen.sh",
    "scripts/check-portable.sh",
    "npm run typecheck",
    "npm test",
    "scripts/pi-check.sh node_modules/.bin/pi",
    "scripts/cargo.sh build --workspace",
    "scripts/cargo.sh nextest run --workspace",
  ]);
});
