import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

// hs-d355 (docs/implementing.md "Running cargo, the Gate, installing"): the Gate is the `verify` script of package.json, in this order, each step stopping the run at its first failure; the cargo steps build with `CPD_COMMIT` ("The version string").
test("the verify script runs the portability check and pi-check, in the documented order", () => {
  const pkg = JSON.parse(readFileSync(join(fileURLToPath(import.meta.url), "..", "..", "..", "package.json"), "utf8")) as { scripts: Record<string, string> };
  assert.equal(pkg.scripts.verify, "scripts/gate-run.sh pnpm run verify:steps");
  // The tests run in a scratch TMPDIR removed after them (hygiene.md), so the steps are not split on every `&&`.
  assert.equal(
    pkg.scripts["verify:steps"],
    ["migrations", "portable", "typecheck", "pi", "fmt", "build", "clippy", "web", "node", "rust"].map((s) => (s === "typecheck" ? "pnpm run typecheck" : `pnpm run verify:${s}`)).join(" && "),
  );
  assert.equal(
    pkg.scripts["verify:web"],
    "pnpm --filter cpd-web-client run typecheck && pnpm --filter cpd-web-client run build && pnpm --filter cpd-web-client run test",
  );
  assert.equal(pkg.scripts.pretypecheck, "scripts/pnpm-fresh.sh");
  assert.equal(pkg.scripts.preverify, "scripts/pnpm-fresh.sh");
  // The word npm; `pnpm` contains the letters, so no plain substring test.
  for (const [name, value] of Object.entries(pkg.scripts)) assert.doesNotMatch(value, /(^|[^a-z])npm([^a-z]|$)/, name);
  assert.equal(pkg.scripts["verify:migrations"], "scripts/check-migrations-frozen.sh");
  assert.equal(pkg.scripts["verify:portable"], "scripts/check-portable.sh");
  // The tests run in a scratch TMPDIR removed after them (hygiene.md).
  assert.equal(pkg.scripts["verify:node"], '(t=$(mktemp -d) && TMPDIR=$t pnpm test; s=$?; rm -rf "$t"; exit $s)');
  assert.equal(pkg.scripts["verify:pi"], "scripts/pi-check.sh node_modules/.bin/pi");
  assert.equal(pkg.scripts["verify:fmt"], "scripts/cargo.sh fmt --all --check");
  assert.equal(pkg.scripts["verify:clippy"], "scripts/cargo.sh clippy --workspace --all-targets -- -D warnings");
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
  const gate = [...pkg.scripts["verify:steps"].matchAll(/pnpm run (\S+)/g)].map((m) => m[1]);
  const workflow = [...ci.matchAll(/^\s*run: scripts\/gate-run\.sh pnpm run (\S+)\s*$/gm)].map((m) => m[1]);
  assert.deepEqual(workflow, gate);
  assert.match(ci, /- name: Dependencies\n\s+run: scripts\/pnpm-fresh\.sh\s*\n/);
  const names = [...ci.matchAll(/^\s*- name: (.+?)\s*$/gm)].map((m) => m[1]);
  const wanted = ["Dependencies", "Migrations frozen", "Portability", "Typecheck", "pi check", "Format", "Rust build", "Clippy", "Web", "Node tests", "Rust tests"];
  assert.deepEqual(names.filter((n) => wanted.includes(n)), wanted);
});

test("package.json pins pnpm and the repository carries a pnpm lockfile and no npm lockfile", () => {
  const root = join(fileURLToPath(import.meta.url), "..", "..", "..");
  const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as { packageManager: string; dependencies: Record<string, string>; devDependencies: Record<string, string> };
  assert.match(pkg.packageManager, /^pnpm@\d+\.\d+\.\d+$/);
  assert.equal(existsSync(join(root, "pnpm-lock.yaml")), true);
  assert.equal(existsSync(join(root, "pnpm-workspace.yaml")), true);
  assert.equal(existsSync(join(root, "package-lock.json")), false);
  assert.ok("chalk" in pkg.devDependencies);
  assert.ok("typebox" in pkg.dependencies);
  const ws = readFileSync(join(root, "pnpm-workspace.yaml"), "utf8");
  assert.match(ws, /^allowBuilds:/m);
  assert.doesNotMatch(ws, /set this to true or false/);
  assert.match(ws, /['"]?@google\/genai['"]?:\s*false/);
  assert.match(ws, /['"]?protobufjs['"]?:\s*false/);
  const lock = readFileSync(join(root, "pnpm-lock.yaml"), "utf8");
  assert.doesNotMatch(lock, /@earendil-works\/pi-tui@?:?\s*1\.4\.0/);
});

test("the CI workflow sets NEXTEST_TEST_THREADS: \"8\" in the env of the Rust tests step only", () => {
  const ci = readFileSync(join(fileURLToPath(import.meta.url), "..", "..", "..", ".github", "workflows", "ci.yml"), "utf8");
  const parts = ci.split(/^\s*- (?=name: )/m);
  const withVar = parts.filter((p) => p.includes("NEXTEST_TEST_THREADS"));
  assert.equal(withVar.length, 1);
  assert.match(withVar[0], /^name: Rust tests\n(?:.*\n)*?\s+env:\n\s+NEXTEST_TEST_THREADS: "8"\n/);
  assert.equal(parts[0].includes("NEXTEST_TEST_THREADS"), false);
});

// A push to main runs the Gate too: two green pull requests can break main together.
test("the CI workflow runs on pull_request, workflow_dispatch and a push to main", () => {
  const ci = readFileSync(join(fileURLToPath(import.meta.url), "..", "..", "..", ".github", "workflows", "ci.yml"), "utf8");
  assert.match(ci, /^on:\n(?:  .*\n)*?  push:\n    branches: \[main\]\n/m);
  assert.match(ci, /^  pull_request:$/m);
  assert.match(ci, /^  workflow_dispatch:$/m);
});
