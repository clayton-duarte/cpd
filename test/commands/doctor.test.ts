import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { locatePi } from "../../src/lib/pi-install.ts";
import { fakeBin, runCli, sandboxEnv, tmpDir } from "../helpers/index.ts";

const config = {
  version: 1, concurrency: 3, retryCap: 2, attemptTimeoutMinutes: 30, askTimeoutMinutes: 60, tickSeconds: 5,
  prPollSeconds: 120, herdrSession: null,
  tiers: { O: "github-copilot/claude-opus-5.5", S: "github-copilot/claude-sonnet-5.5", H: "github-copilot/claude-haiku-4.5" },
  leadTier: "O3", defaultReviewTier: "S4", reviewMode: "fork", leadPiPackages: [],
};

function env(opts: { config?: unknown; pathPrefix?: string[] } = {}) {
  const e = sandboxEnv({ PI_CODING_AGENT_DIR: locatePi().agentDir }, opts.pathPrefix);
  if (opts.config !== undefined) {
    mkdirSync(e.CPD_HOME!, { recursive: true });
    writeFileSync(join(e.CPD_HOME!, "config.json"), JSON.stringify(opts.config));
  }
  return e;
}

const byName = (checks: any[], name: string) => checks.find((c) => c.name === name);
const NAMES = ["node", "bd", "wt", "herdr", "pi", "gh", "config", "tiers"];

test("on this machine every check passes, with found and expected versions", async () => {
  const r = await runCli(["doctor"], { env: env({ config }) });
  assert.equal(r.code, 0, r.stdout + r.stderr);
  assert.equal(r.json.ok, true);
  assert.deepEqual(r.json.checks.map((c: any) => c.name), NAMES);
  assert.ok(r.json.checks.every((c: any) => c.ok));
  for (const [n, v] of [["bd", "1.3.1"], ["wt", "0.80.0"], ["herdr", "0.9.3"]] as const) {
    assert.equal(byName(r.json.checks, n).found, v);
    assert.equal(byName(r.json.checks, n).expected, v);
  }
  assert.match(byName(r.json.checks, "pi").found, /^1\.\d+\.\d+$/);
  assert.match(byName(r.json.checks, "node").found, /type stripping/);
});

test("a wrong tool version fails that check only, exit 1, with found and expected", async () => {
  const bin = fakeBin("bd", `echo "bd version 9.9.9 (Fake)"`);
  const r = await runCli(["doctor"], { env: env({ config, pathPrefix: [bin.dir] }) });
  assert.equal(r.code, 1);
  assert.equal(r.json.ok, false);
  assert.equal(r.json.error.code, "doctor_failed");
  const bd = byName(r.json.checks, "bd");
  assert.deepEqual([bd.ok, bd.found, bd.expected], [false, "9.9.9", "1.3.1"]);
  assert.deepEqual(r.json.checks.filter((c: any) => !c.ok).map((c: any) => c.name), ["bd"]);
});

test("a missing config fails config and tiers", async () => {
  const r = await runCli(["doctor"], { env: env() });
  assert.equal(r.code, 1);
  assert.equal(byName(r.json.checks, "config").ok, false);
  assert.match(byName(r.json.checks, "config").message, /config_missing/);
  assert.equal(byName(r.json.checks, "tiers").ok, false);
});

test("a tier model that pi does not know fails tiers", async () => {
  const r = await runCli(["doctor"], { env: env({ config: { ...config, tiers: { ...config.tiers, S: "github-copilot/nope" } } }) });
  assert.equal(r.code, 1);
  assert.equal(byName(r.json.checks, "config").ok, true);
  assert.match(byName(r.json.checks, "tiers").message, /tier_model_unknown.*nope/);
});

test("missing tools (pi, gh) fail their checks and say why", async () => {
  const dir = tmpDir("cpd-bin-");
  fakeBin("bd", `echo "bd version 1.3.1 (Fake)"`, dir);
  fakeBin("wt", `echo "wt v0.80.0"`, dir);
  fakeBin("herdr", `echo "herdr 0.9.3"`, dir);
  const e = env({ config });
  e.PATH = dir;
  const r = await runCli(["doctor"], { env: e });
  assert.equal(r.code, 1);
  assert.deepEqual(r.json.checks.filter((c: any) => !c.ok).map((c: any) => c.name), ["pi", "gh", "tiers"]);
  assert.ok(byName(r.json.checks, "pi").message);
  assert.ok(byName(r.json.checks, "gh").message);
});

test("cpd doctor rejects unknown flags (exit 2)", async () => {
  const r = await runCli(["doctor", "--nope"], { env: env({ config }) });
  assert.equal(r.code, 2);
  assert.equal(r.json.error.code, "usage");
});
