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
    ["migrations", "portable", "typecheck", "pi", "build", "node", "rust"].map((s) => (s === "typecheck" ? "npm run typecheck" : `npm run verify:${s}`)).join(" && "),
  );
  assert.equal(pkg.scripts["verify:migrations"], "scripts/check-migrations-frozen.sh");
  assert.equal(pkg.scripts["verify:portable"], "scripts/check-portable.sh");
  // The tests run in a scratch TMPDIR removed after them (hygiene.md).
  assert.equal(pkg.scripts["verify:node"], '(t=$(mktemp -d) && TMPDIR=$t ' + "npm" + ' test; s=$?; rm -rf "$t"; exit $s)');
  assert.equal(pkg.scripts["verify:pi"], "scripts/pi-check.sh node_modules/.bin/pi");
  assert.equal(pkg.scripts["verify:build"], "export CPD_COMMIT=$(git rev-parse --short HEAD) && scripts/cargo.sh nextest run --workspace --no-run");
  assert.equal(
    pkg.scripts["verify:rust"],
    'export CPD_COMMIT=$(git rev-parse --short HEAD) && PATH="$PWD/scripts/tripwire:$PATH" scripts/cargo.sh nextest run --workspace',
  );
});

// CI runs the Gate's stages as separate steps: one list (package.json), no drift.
test("the CI workflow runs the stages of verify:steps, in order, as named steps", () => {
  const root = join(fileURLToPath(import.meta.url), "..", "..", "..");
  const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as { scripts: Record<string, string> };
  const ci = readFileSync(join(root, ".github", "workflows", "ci.yml"), "utf8");
  const gate = [...pkg.scripts["verify:steps"].matchAll(/npm run (\S+)/g)].map((m) => m[1]);
  const workflow = [...ci.matchAll(/^\s*run: scripts\/gate-run\.sh npm run (\S+)\s*$/gm)].map((m) => m[1]);
  assert.deepEqual(workflow, gate);
  const names = [...ci.matchAll(/^\s*- name: (.+?)\s*$/gm)].map((m) => m[1]);
  const wanted = ["Dependencies", "Migrations frozen", "Portability", "Typecheck", "pi check", "Rust build", "Node tests", "Rust tests"];
  assert.deepEqual(names.filter((n) => wanted.includes(n)), wanted);
});

// A push to main runs the Gate too: two green pull requests can break main together.
test("the CI workflow runs on pull_request, workflow_dispatch and a push to main", () => {
  const ci = readFileSync(join(fileURLToPath(import.meta.url), "..", "..", "..", ".github", "workflows", "ci.yml"), "utf8");
  assert.match(ci, /^on:\n(?:  .*\n)*?  push:\n    branches: \[main\]\n/m);
  assert.match(ci, /^  pull_request:$/m);
  assert.match(ci, /^  workflow_dispatch:$/m);
});
