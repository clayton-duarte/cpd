import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

// hs-d355 (docs/implementing.md "Running cargo, the Gate, installing"): the Gate is the `verify` script of package.json, in this order, each step stopping the run at its first failure; the cargo steps build with `CPD_COMMIT` ("The version string").
test("the verify script runs the portability check and pi-check, in the documented order", () => {
  const pkg = JSON.parse(readFileSync(join(fileURLToPath(import.meta.url), "..", "..", "..", "package.json"), "utf8")) as { scripts: Record<string, string> };
  assert.equal(pkg.scripts.verify, "scripts/gate-run.sh npm run verify:steps");
  // The tests run in a scratch TMPDIR removed after them (hygiene.md), so the steps are not split on every `&&`.
  assert.equal(
    pkg.scripts["verify:steps"],
    [
      "scripts/check-migrations-frozen.sh",
      "scripts/check-portable.sh",
      "npm run typecheck",
      '(t=$(mktemp -d) && TMPDIR=$t ' + "npm" + ' test; s=$?; rm -rf "$t"; exit $s)',
      "scripts/pi-check.sh node_modules/.bin/pi",
      "export CPD_COMMIT=$(git rev-parse --short HEAD)",
      "scripts/cargo.sh nextest run --workspace",
    ].join(" && "),
  );

});
