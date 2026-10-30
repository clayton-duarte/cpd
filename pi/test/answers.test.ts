import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { CpdError } from "../lib/cpd.ts";
import { PROTOCOL } from "../lib/event.ts";
import { policySection } from "../lib/policy.ts";
import { parseSession } from "../lib/run.ts";
import { parsePending } from "../lib/updates.ts";
import { answers } from "./answers.ts";
import { Fixture } from "./fake.ts";

// hs-d355 (docs/implementing.md "Checking a pi version"): the answers the adapter parses at activation have one source, `pi/test/answers.ts`,
// used by the tests' fake `cpd` and by pi-check's fake. A field the adapter starts requiring is added there once; until then these tests fail.

const KEYS = ["--version", "context-pending", "policy-show", "session-show"];
const answersFile = join(fileURLToPath(import.meta.url), "..", "answers.ts");
const entry = (key: string): Record<string, unknown> => {
  const answer = (answers() as Record<string, unknown>)[key];
  assert.ok(answer !== undefined && typeof answer === "object", `answers() has no entry for ${key}`);
  return answer as Record<string, unknown>;
};

test("answers() holds exactly what the adapter calls at activation, keyed as Fixture.respond", () => {
  assert.deepEqual(Object.keys(answers()).sort(), KEYS);
});

test("the session-show answer is accepted by the adapter's own parser, and each field the parser requires is in it", () => {
  const show = entry("session-show");
  assert.doesNotThrow(() => parseSession(show));
  const session = show.session as Record<string, unknown>;
  for (const field of ["role", "context", "plan", "plan_lead", "registered_event", "name"]) {
    assert.ok(field in session, `the answer lacks \`${field}\``);
    const without = { ...show, session: { ...session } };
    delete (without.session as Record<string, unknown>)[field];
    assert.throws(() => parseSession(without), (e: unknown) => e instanceof CpdError && e.code === "bad_output", `the adapter accepts an answer without \`${field}\``);
  }
});

test("the policy-show, context-pending and --version answers are the shapes the adapter parses", () => {
  assert.doesNotThrow(() => policySection(entry("policy-show")));
  assert.doesNotThrow(() => parsePending(entry("context-pending")));
  assert.equal(entry("--version").ok, true);
  assert.equal(entry("--version").protocol, PROTOCOL, "the protocol number is the adapter's own constant");
});

test("the Fixture's default respond calls are the answers", async () => {
  const f = new Fixture();
  try {
    for (const key of KEYS) {
      assert.deepEqual(f.cpd.responses.get(key)?.stdout, entry(key), `the Fixture answers ${key} with its own copy`);
    }
  } finally {
    await f.close();
  }
});

const cli = (...argv: string[]): unknown => {
  const out = execFileSync(process.execPath, [answersFile, ...argv], { encoding: "utf8", timeout: 20_000 });
  assert.notEqual(out.trim(), "", `node pi/test/answers.ts ${argv.join(" ")} printed nothing`);
  return JSON.parse(out);
};

test("node pi/test/answers.ts <argv> prints the entry of the key of the argv as JSON, and {ok:true} for a command without one", () => {
  assert.deepEqual(cli("session", "show", "596d007f-6775-4134-b77a-b356cc523842"), entry("session-show"));
  assert.deepEqual(cli("policy", "show"), entry("policy-show"));
  assert.deepEqual(cli("context", "pending"), entry("context-pending"));
  assert.deepEqual(cli("--version"), entry("--version"));
  assert.deepEqual(cli("plan", "show", "p1"), { ok: true });
});
