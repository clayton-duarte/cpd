import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, symlinkSync } from "node:fs";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { runCli, tmpDir } from "../helpers/index.ts";
import { cliSandbox, spec, writeSpec } from "../ledger/helpers.ts";

const { env, repo, cli } = cliSandbox();
const wf = "jobedit";

// PATH with `bd` but without `pi`: the Tier catalog cannot be loaded.
const bin = join(tmpDir(), "bin");
mkdirSync(bin);
symlinkSync(execFileSync("which", ["bd"], { encoding: "utf8" }).trim(), join(bin, "bd"));
const noPi = (args: string[]) => runCli(args, { env: { ...env, PATH: bin } });

test("job edit loads the Tier catalog only for --tier or --review-tier", async () => {
  assert.equal((await cli(["workflow", "init", "--wf", wf, "--repo", repo, "--title", "Edit"])).code, 0);
  assert.equal((await cli(["workflow", "phase", "plan", "--wf", wf])).code, 0);
  const file = writeSpec(tmpDir(), "spec.json", spec());
  assert.equal((await cli(["job", "add", "--wf", wf, "--key", "k", "--title", "t", "--tier", "S3", "--spec", file])).code, 0);

  const ok = await noPi(["job", "edit", "k", "--wf", wf, "--title", "x"]);
  assert.equal(ok.code, 0, ok.stdout + ok.stderr);
  assert.equal(ok.json.job.title, "x");

  const bad = await noPi(["job", "edit", "k", "--wf", wf, "--tier", "S4"]);
  assert.notEqual(bad.code, 0);
  assert.equal(bad.json.ok, false);
  const bad2 = await noPi(["job", "edit", "k", "--wf", wf, "--review-tier", "S4"]);
  assert.equal(bad2.json.error.code, bad.json.error.code);

  assert.equal((await cli(["job", "edit", "k", "--wf", wf, "--tier", "S4"])).code, 0, "works with pi on PATH");
});
