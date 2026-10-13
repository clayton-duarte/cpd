import { test } from "node:test";
import assert from "node:assert/strict";
import { tmpDir } from "../helpers/index.ts";
import { bdJson, cliSandbox, jobId, rawBd, spec, writeSpec } from "../ledger/helpers.ts";

const { repo, cli } = cliSandbox();
const wf = "showbroken";

test("workflow show lists a hand-broken Job under invalid and still returns the rest; job show stays fail-loud", async () => {
  assert.equal((await cli(["workflow", "init", "--wf", wf, "--repo", repo, "--title", "Show"])).code, 0);
  assert.equal((await cli(["workflow", "phase", "plan", "--wf", wf])).code, 0);
  const file = writeSpec(tmpDir(), "spec.json", spec());
  for (const k of ["good", "bad"]) {
    assert.equal((await cli(["job", "add", "--wf", wf, "--key", k, "--title", k, "--tier", "S3", "--spec", file])).code, 0);
  }
  const happy = await cli(["workflow", "show", "--wf", wf]);
  assert.equal("invalid" in happy.json, false, "happy-path output is unchanged");

  const id = await jobId(wf, "bad");
  const r = await rawBd(wf, ["update", id, "--metadata", JSON.stringify({ key: "bad", tier: 7 })], "intruder");
  assert.equal(r.code, 0, r.stderr);

  const show = await cli(["workflow", "show", "--wf", wf]);
  assert.equal(show.code, 0, show.stdout + show.stderr);
  assert.deepEqual(show.json.jobs.map((j: any) => j.key), ["good"]);
  assert.equal(show.json.invalid.length, 1);
  assert.equal(show.json.invalid[0].id, id);
  assert.equal(show.json.invalid[0].key, "bad");
  assert.ok(show.json.invalid[0].issues.length > 0);
  assert.ok(show.json.tamper.some((t: any) => t.issue === id && t.actor === "intruder"));

  const one = await cli(["job", "show", "bad", "--wf", wf]);
  assert.equal(one.code, 1);
  assert.equal(one.json.error.code, "ledger_invalid");
  assert.equal((await cli(["job", "list", "--wf", wf])).json.error.code, "ledger_invalid");
  void bdJson;
});
