import assert from "node:assert/strict";
import { test } from "node:test";
import { CpdError } from "../../lib/cpd.ts";
import { parseConfig, reportPath, storePath } from "../../durable-worker/config.ts";

const ENV = { CPD_HOME: "/h/.cpd", CPD_SESSION: "s1", CPD_PLAN: "p1", CPD_ATTEMPT: "j1-a1" };
const ARGV = ["--model", "anthropic/sonnet:high", "--spec", "/specs/j1-a1.md"];

/** The `usage` CpdError `fn` throws; any other throw (a stub's) is rethrown. */
function usage(fn: () => unknown): CpdError {
  try {
    fn();
  } catch (e) {
    if (!(e instanceof CpdError)) throw e;
    assert.equal(e.code, "usage");
    return e;
  }
  throw new assert.AssertionError({ message: "did not throw" });
}

test("parseConfig reads the Attempt's identity from the environment and the model and Spec from argv", () => {
  assert.deepEqual(parseConfig(ARGV, ENV, "/wt"), {
    home: "/h/.cpd",
    session: "s1",
    plan: "p1",
    attempt: "j1-a1",
    cwd: "/wt",
    spec: "/specs/j1-a1.md",
    model: "anthropic/sonnet:high",
  });
});

test("parseConfig names the missing variable", () => {
  for (const name of Object.keys(ENV)) {
    const env: Record<string, string | undefined> = { ...ENV, [name]: undefined };
    assert.match(usage(() => parseConfig(ARGV, env, "/wt")).message, new RegExp(name), name);
  }
});

test("parseConfig refuses a missing --model or --spec", () => {
  assert.match(usage(() => parseConfig(["--spec", "/s.md"], ENV, "/wt")).message, /--model/);
  assert.match(usage(() => parseConfig(["--model", "m"], ENV, "/wt")).message, /--spec/);
});

test("storePath and reportPath are under run/<plan>/durable/<attempt>", () => {
  assert.equal(storePath("/h/.cpd", "p1", "j1-a1"), "/h/.cpd/run/p1/durable/j1-a1.sqlite");
  assert.equal(reportPath("/h/.cpd", "p1", "j1-a1"), "/h/.cpd/run/p1/durable/j1-a1.report.json");
});
