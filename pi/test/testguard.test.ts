import assert from "node:assert/strict";
import { test } from "node:test";
import { reasonText, unfilteredTestRun } from "../lib/testguard.ts";
import { Fixture, shown, waitFor } from "./fake.ts";

// commands.md §15.12: a Worker's bash call that runs a full unfiltered test suite is refused.

const REFUSED = [
  "scripts/cargo.sh test --workspace",
  "scripts/cargo.sh nextest run --workspace",
  "cargo test --workspace",
  "cargo test --workspace -p cpd",
  "cargo nextest run --workspace --test plan_overlap",
  "cargo test",
  "cargo test --release",
  "cargo test -- --nocapture",
  "cargo nextest run",
  "cargo nextest run --no-fail-fast",
  "scripts/cargo.sh nextest run",
  "scripts/cargo.sh test",
  "timeout 600 scripts/cargo.sh nextest run",
  "timeout -s KILL 600 cargo test --workspace",
  "RUSTC_WRAPPER= scripts/cargo.sh test",
  "cargo +nightly test",
  "cargo test -j 4",
  "cargo test --test-threads 1",
  "npm test",
  "npm run test",
  "npm t",
  "npm test --silent",
  "npm test --",
  "npm test -- --test-reporter=spec",
  "timeout 300 npm test",
  "npm run verify",
  "npm run verify --silent",
  "timeout 900 npm run verify",
  "npm run-script verify",
  "cd pi && npm test",
  "pnpm test",
  "pnpm run test",
  "pnpm t",
  "pnpm tst",
  "pnpm test --silent",
  "pnpm test --",
  "pnpm test -- --test-reporter=spec",
  "timeout 300 pnpm test",
  "pnpm run verify",
  "pnpm run verify --silent",
  "timeout 900 pnpm run verify",
  "pnpm run-script verify",
  "pnpm verify",
  "cd pi && pnpm test",
  "echo go; scripts/cargo.sh test --workspace | tail",
];

const ALLOWED = [
  "scripts/cargo.sh nextest run -p cpd --test plan_overlap",
  "scripts/cargo.sh nextest run --package cpd",
  "timeout 300 scripts/cargo.sh nextest run -p cpd --test plan_overlap",
  "cargo test -p cpd-core spec::",
  "cargo test -pcpd-core",
  "cargo test --lib",
  "cargo test --bin cpd",
  "cargo test --test plan_overlap",
  "cargo test --test=plan_overlap",
  "cargo test spec::",
  "cargo test -- spec::",
  "cargo test --release spec::",
  "cargo nextest run -E 'test(spec)'",
  "cargo nextest run --filterset 'test(spec)'",
  "cargo nextest run spec::",
  "cargo nextest run -j 4 spec::",
  "node --test pi/test/x.test.ts",
  "node --test pi/test/testguard.test.ts pi/test/noblock.test.ts",
  "npm test -- pi/test/x.test.ts",
  "npm run test -- pi/test/x.test.ts",
  "npm test pi/test/x.test.ts",
  "npm run typecheck",
  "npm run build",
  "npm install",
  "pnpm test -- pi/test/x.test.ts",
  "pnpm run test -- pi/test/x.test.ts",
  "pnpm test pi/test/x.test.ts",
  "pnpm run typecheck",
  "pnpm run build",
  "pnpm install",
  "pnpm install --frozen-lockfile",
  "pnpm exec tsc --noEmit",
  "pnpm add chalk",
  "cargo build",
  "scripts/cargo.sh build --workspace",
  "cargo check --workspace",
  "cargo fmt",
  "cargo nextest list",
  "ls",
  "",
  "grep -r 'cargo test' docs",
  'echo "npm run verify"',
  "echo cargo test",
  "# cargo test",
  "git log --grep 'npm test'",
  "grep -r 'pnpm test' docs",
  'echo "pnpm run verify"',
  "# pnpm test",
  "git log --grep 'pnpm test'",
];

test("a full unfiltered test run is refused, however it is wrapped", () => {
  for (const c of REFUSED) assert.notEqual(unfilteredTestRun(c), undefined, `${c} must be refused`);
});

test("a targeted run, a build and other commands run", () => {
  for (const c of ALLOWED) assert.equal(unfilteredTestRun(c), undefined, `${c} must run`);
});

test("the reason is one line naming the rule and what matched", () => {
  const r = reasonText(unfilteredTestRun("scripts/cargo.sh test --workspace")!);
  assert.equal(r, "φ: a Worker runs only the tests its change touches; the full suite is the Gate's, run by the Workflow's open Job; refused: cargo.sh test --workspace");
  assert.equal(r.includes("\n"), false);
  assert.equal(unfilteredTestRun("timeout 60 cargo nextest run"), "cargo nextest run");
  assert.equal(unfilteredTestRun("npm test"), "npm test");
  assert.equal(unfilteredTestRun("npm run verify"), "npm run verify");
  assert.equal(unfilteredTestRun("pnpm test"), "pnpm test");
  assert.equal(unfilteredTestRun("pnpm t"), "pnpm test");
  assert.equal(unfilteredTestRun("pnpm run test"), "pnpm run test");
  assert.equal(unfilteredTestRun("pnpm run verify"), "pnpm run verify");
  assert.equal(unfilteredTestRun("pnpm run-script verify"), "pnpm run verify");
  assert.equal(unfilteredTestRun("pnpm verify"), "pnpm run verify");
  assert.equal(unfilteredTestRun("timeout 60 pnpm test"), "pnpm test");
});

// ---- through the adapter ----

async function started(role: string) {
  const f = new Fixture();
  f.respond("session-show", shown({ role, plan: role === "manager" ? null : "p1", plan_lead: role === "lead" }));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  return f;
}

const call = async (f: Fixture, command: string, toolName = "bash") => (await f.pi.fireWith("tool_call", { toolName, toolCallId: "t", input: { command } }))[0] as { block?: boolean; reason?: string } | undefined;

test("a Worker's full test runs get block and the rule's reason; the targeted ones run", async (t) => {
  const f = await started("worker");
  t.after(() => f.close());
  for (const c of ["scripts/cargo.sh test --workspace", "cargo nextest run", "npm test", "npm run verify", "pnpm test", "pnpm run verify"]) {
    const r = await call(f, c);
    assert.equal(r?.block, true, c);
    assert.match(r?.reason ?? "", /^φ: a Worker runs only the tests its change touches; the full suite is the Gate's, run by the Workflow's open Job; refused: /);
  }
  for (const c of ["scripts/cargo.sh nextest run -p cpd --test plan_overlap", "cargo test -p cpd-core spec::", "node --test pi/test/x.test.ts", "npm test -- pi/test/x.test.ts", "npm run typecheck", "pnpm test -- pi/test/x.test.ts", "pnpm run typecheck", "cargo build"])
    assert.equal(await call(f, c), undefined, c);
});

test("a Worker's other tools are never refused", async (t) => {
  const f = await started("worker");
  t.after(() => f.close());
  assert.equal(await call(f, "npm run verify", "read"), undefined);
});

test("a Lead's identical calls are not refused", async (t) => {
  const f = await started("lead");
  t.after(() => f.close());
  // the `timeout N` entries are left out: a Lead's wait over 30 s is refused by noblock (commands.md 15.11)
  for (const c of REFUSED.filter((c) => !c.startsWith("timeout "))) assert.equal(await call(f, c), undefined, c);
});

test("a Manager is not refused for a test run (its own guard is the no-block one)", async (t) => {
  const f = await started("manager");
  t.after(() => f.close());
  for (const c of ["scripts/cargo.sh test --workspace", "npm run verify"]) assert.equal(await call(f, c), undefined, c);
});

test("before session_start no call is refused", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.load();
  assert.equal(await call(f, "npm run verify"), undefined);
});

test("a heredoc body is not read as a test run, the command after it is", () => {
  assert.equal(unfilteredTestRun("cat <<EOF\ncargo test --workspace\nnpm test\nEOF"), undefined);
  assert.equal(unfilteredTestRun("cat <<EOF\nx\nEOF\ncargo test --workspace"), "cargo test --workspace");
});
