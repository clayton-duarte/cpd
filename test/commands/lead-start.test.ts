import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { locatePi } from "../../src/lib/pi-install.ts";
import { baseConfig } from "../herdr/setup.ts";
import { fakeBin, runCli, sandboxEnv, tmpDir } from "../helpers/index.ts";

const REPO = fileURLToPath(new URL("../../", import.meta.url));
const REAL = locatePi();

/** A fake pi agent dir: `bin/pi` is a recording script, `install/` points at the real release so the Tier catalog still loads. */
function fakePi() {
  const agentDir = tmpDir("cpd-agent-");
  mkdirSync(join(agentDir, "bin"));
  mkdirSync(join(agentDir, "install", "releases"), { recursive: true });
  writeFileSync(join(agentDir, "install", "current-version"), REAL.version + "\n");
  symlinkSync(REAL.releaseDir, join(agentDir, "install", "releases", REAL.version));
  const envFile = join(agentDir, "env.txt");
  const pi = fakeBin("pi", `env > '${envFile}'`, join(agentDir, "bin"));
  return { agentDir, pi, envFile };
}

function setup(config: object = {}) {
  const { agentDir, pi, envFile } = fakePi();
  const env = sandboxEnv({ HERDR_ENV: "1", PI_CODING_AGENT_DIR: REAL.agentDir }, [pi.dir]);
  mkdirSync(env.CPD_HOME!, { recursive: true });
  writeFileSync(join(env.CPD_HOME!, "config.json"), JSON.stringify({ ...baseConfig, ...config }));
  return { agentDir, pi, envFile, env };
}

test("outside herdr: not_in_herdr, exit 3, and pi is not run", async () => {
  const { env, pi } = setup();
  delete env.HERDR_ENV;
  const r = await runCli(["lead", "start"], { env });
  assert.equal(r.code, 3);
  assert.equal(r.json.error.code, "not_in_herdr");
  assert.deepEqual(pi.calls(), []);
});

test("execs pi in place with the Lead model, the adapter and every package, and the plannotator env", async () => {
  const { env, pi, envFile } = setup({ leadPiPackages: ["npm:a@1.0.0", "npm:b@2.0.0"] });
  const r = await runCli(["lead", "start"], { env });
  assert.equal(r.code, 0, r.stdout + r.stderr);
  assert.equal(r.stdout, "", "exec replaced the process: cpd printed no JSON");
  const [argv] = pi.calls();
  assert.match(argv![1]!, /^github-copilot\/claude-opus-5\.5:high$/); // leadTier O3
  assert.deepEqual(argv, ["--model", argv![1], "-e", join(REPO, "pi"), "-e", "npm:a@1.0.0", "-e", "npm:b@2.0.0"]);
  const childEnv = Object.fromEntries(readFileSync(envFile, "utf8").trim().split("\n").map((l) => [l.slice(0, l.indexOf("=")), l.slice(l.indexOf("=") + 1)]));
  assert.equal(childEnv.CPD_HOME, env.CPD_HOME);
  assert.equal(childEnv.PLANNOTATOR_DATA_DIR, join(env.CPD_HOME!, "plannotator"));
  assert.equal(childEnv.PLANNOTATOR_UPDATE_CHECK, "0");
  assert.equal(childEnv.HERDR_ENV, "1");
});

test("pi's exit code is the process exit code (it was exec'd, not wrapped)", async () => {
  const { agentDir, env } = setup();
  fakeBin("pi", "exit 7", join(agentDir, "bin")); // replaces the recording script
  const r = await runCli(["lead", "start"], { env });
  assert.equal(r.code, 7);
});

test("a bad config fails loud before anything is exec'd; unknown flags are usage errors", async () => {
  const a = setup({ leadTier: "O9" });
  const r1 = await runCli(["lead", "start"], { env: a.env });
  assert.equal(r1.code, 1);
  assert.equal(r1.json.error.code, "config_invalid");
  assert.deepEqual(a.pi.calls(), []);
  const b = setup();
  const r2 = await runCli(["lead", "start", "--bogus"], { env: b.env });
  assert.deepEqual([r2.code, r2.json.error.code], [2, "usage"]);
});
