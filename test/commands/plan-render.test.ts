import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { planFingerprint } from "../../src/ledger/plan.ts";
import { runCli, tmpDir } from "../helpers/index.ts";
import { cliSandbox, spec, writeSpec } from "../ledger/helpers.ts";

const { env, repo, cli } = cliSandbox();
const dir = tmpDir();
const wf = "plancli";

test("setup: a Workflow in phase plan with two Jobs", async () => {
  assert.equal((await cli(["workflow", "init", "--wf", wf, "--repo", repo, "--title", "Plan CLI"])).code, 0);
  assert.equal((await cli(["workflow", "phase", "plan", "--wf", wf])).code, 0);
  const f = writeSpec(dir, "s.json", spec());
  for (const [key, needs] of [["a", []], ["b", ["a"]]] as const) {
    const r = await cli(["job", "add", "--wf", wf, "--key", key, "--title", `Job ${key}`, "--tier", "S3", "--spec", f, ...(needs.length ? ["--needs", needs.join(",")] : [])]);
    assert.equal(r.code, 0, r.stdout);
  }
});

test("plan render writes <wf>/plan.md by default and returns {ok, path, fingerprint}", async () => {
  const r = await cli(["plan", "render", "--wf", wf]);
  assert.equal(r.code, 0, r.stdout + r.stderr);
  assert.equal(r.json.ok, true);
  assert.equal(r.json.path, join(env.CPD_HOME!, "workflows", wf, "plan.md"));
  assert.deepEqual(Object.keys(r.json).sort(), ["fingerprint", "ok", "path"]);
  process.env.CPD_HOME = env.CPD_HOME;
  process.env.HOME = env.HOME;
  assert.equal(r.json.fingerprint, await planFingerprint(wf));
  const md = readFileSync(r.json.path, "utf8");
  assert.match(md, /^# Plan: Plan CLI\n\nPhase: `plan`\n/);
  assert.equal(md.split("\n").filter((l) => l.includes("-->")).length, 1);
});

test("--out writes there (parents created) and CPD_WF selects the Workflow", async () => {
  const out = join(dir, "deep", "plan.md");
  const r = await cli(["plan", "render", "--out", out]);
  assert.equal(r.code, 2, "no workflow selected");
  assert.equal(r.json.error.code, "workflow_required");
  const ok = await runCli(["plan", "render", "--out", out], { env: { ...env, CPD_WF: wf } });
  assert.equal(ok.code, 0, ok.stdout);
  assert.equal(ok.json.path, out);
  assert.ok(existsSync(out));
});

test("the fingerprint changes when a draft changes and is stable otherwise", async () => {
  const a = (await cli(["plan", "render", "--wf", wf])).json.fingerprint;
  assert.equal((await cli(["plan", "render", "--wf", wf])).json.fingerprint, a);
  assert.equal((await cli(["job", "edit", "b", "--wf", wf, "--title", "Renamed"])).code, 0);
  assert.notEqual((await cli(["plan", "render", "--wf", wf])).json.fingerprint, a);
});

test("usage and not-found errors", async () => {
  assert.equal((await cli(["plan", "render", "--wf", wf, "--bogus"])).code, 2);
  assert.equal((await cli(["plan", "render", "--wf", wf, "extra"])).code, 2);
  const r = await cli(["plan", "render", "--wf", "nope"]);
  assert.equal(r.code, 4);
  assert.equal(r.json.error.code, "workflow_not_found");
});
