import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, symlinkSync } from "node:fs";
import { join } from "node:path";
import { writeFileSync } from "node:fs";
import { version } from "../src/cli.ts";
import { CLI, runCli, tmpDir } from "./helpers/index.ts";

// Commands listed in contract §11.
const EXPECTED = [
  "doctor", "tiers", "lead start", "lead attach",
  "workflow init", "workflow show", "workflow phase",
  "job add", "job edit", "job list", "job show", "job cancel", "job retry", "job dispatch",
  "plan render", "plan decision", "plan lock", "watch",
  "attempt run", "attempt submit", "attempt read", "attempt list", "attempt show",
  "review decide", "ask", "answer", "pr open", "pr poll", "pr rebase",
  "session rename",
];

test("--help lists every contract command", async () => {
  const r = await runCli(["--help"]);
  assert.equal(r.code, 0);
  assert.equal(r.json.ok, true);
  assert.equal(r.json.commands.length, 30);
  assert.deepEqual([...r.json.commands].sort(), [...EXPECTED].sort());
});

/** A command file that no longer mentions `not_implemented` has been implemented by its Job. */
const isStub = (cmd: string) =>
  readFileSync(new URL(`../src/commands/${cmd.replace(" ", "-")}.ts`, import.meta.url), "utf8").includes("not_implemented");

for (const cmd of EXPECTED) {
  test(`stub: cpd ${cmd} -> not_implemented`, { skip: !isStub(cmd) && "implemented" }, async () => {
    const r = await runCli(cmd.split(" "));
    assert.equal(r.code, 1);
    assert.equal(r.json.ok, false);
    assert.equal(r.json.error.code, "not_implemented");
    assert.ok(r.json.error.message.includes(cmd));
  });
}

test("unknown command exits 2", async () => {
  for (const args of [["nope"], ["job", "nope"], ["job"], ["constructor"], ["toString"], ["hasOwnProperty"], ["__proto__"], ["job", "__proto__"], ["job", "constructor"], ["job add"], ["job nope"]]) {
    const r = await runCli(args);
    assert.equal(r.code, 2);
    assert.equal(r.json.error.code, "unknown_command");
  }
});

test("--version", async () => {
  const r = await runCli(["--version"]);
  assert.equal(r.code, 0);
  assert.equal(r.json.ok, true);
  assert.equal(typeof r.json.version, "string");
});

test("works through a symlink", async () => {
  const link = join(tmpDir(), "cpd");
  symlinkSync(CLI, link);
  const r = await runCli(["--version"], { bin: link });
  assert.equal(r.code, 0);
  assert.equal(r.json.ok, true);
});

test("version() returns the version of the real package.json", () => {
  const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
  assert.equal(version(), pkg.version);
});

test("version() fails clearly on a malformed package.json", () => {
  const dir = tmpDir();
  for (const [name, body] of [["noversion.json", '{"name":"x"}'], ["number.json", '{"version":1}'], ["junk.json", "{"]] as const) {
    writeFileSync(join(dir, name), body);
    assert.throws(() => version(new URL(`file://${join(dir, name)}`)), (e: any) => e.code === "bad_package" && e.exitCode === 1, name);
  }
});
