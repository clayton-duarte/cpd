import assert from "node:assert/strict";
import { test } from "node:test";
import { CpdError } from "../../lib/cpd.ts";
import { parseConfig, storePath } from "../../durable-host/config.ts";

const ENV = { CPD_HOME: "/h/.cpd" };
const ARGV = ["--session", "s1", "--model", "anthropic/sonnet:high"];

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

test("parseConfig reads --session, --model and CPD_HOME", () => {
  assert.deepEqual(parseConfig(ARGV, ENV, "/wt"), { home: "/h/.cpd", session: "s1", cwd: "/wt", model: "anthropic/sonnet:high" });
});

test("parseConfig refuses a missing --session with a usage error", () => {
  assert.match(usage(() => parseConfig(["--model", "m"], ENV, "/wt")).message, /--session/);
});

test("parseConfig refuses a missing --model with a usage error", () => {
  assert.match(usage(() => parseConfig(["--session", "s1"], ENV, "/wt")).message, /--model/);
});

test("parseConfig refuses a missing CPD_HOME with a usage error", () => {
  assert.match(usage(() => parseConfig(ARGV, {}, "/wt")).message, /CPD_HOME/);
});

test("storePath is run/host/<session>.sqlite under the home", () => {
  assert.equal(storePath("/h/.cpd", "s1"), "/h/.cpd/run/host/s1.sqlite");
});
