import assert from "node:assert/strict";
import { test } from "node:test";
import { CpdError } from "../lib/cpd.ts";
import { POLICY_SECTION, policySection } from "../lib/policy.ts";
import { Fixture, policyShow, shown } from "./fake.ts";
import { adapterErrors } from "./lead-errors.ts";

/** Runs a start and a `before_agent_start`; returns the sections pi would hand to the prompt. */
async function sectionsAfterStart(f: Fixture): Promise<Record<string, string>> {
  f.load();
  await f.pi.fire("session_start");
  const sections: Record<string, string> = { preamble: "x" };
  await f.pi.fire("before_agent_start", { messageType: "user", prompt: "hi", systemPromptOptions: { sections } });
  return sections;
}

test("a Lead with a pr and an instructions text gets the exact section", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("policy-show", policyShow("lead", { pr: "These repos run no GitHub Actions.", instructions: "Be brief." }));
  const sections = await sectionsAfterStart(f);
  assert.deepEqual(sections, {
    preamble: "x",
    [POLICY_SECTION]: "## pr (owner:/p/o.toml)\nThese repos run no GitHub Actions.\n\n## instructions (owner:/p/o.toml)\nBe brief.",
  });
  assert.deepEqual(f.policyCalls(), [["policy", "show"]], "no flag: the adapter never picks a key");
  assert.deepEqual(adapterErrors(f), []);
});

test("no section when every text is empty, and nothing else is changed", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  assert.deepEqual(await sectionsAfterStart(f), { preamble: "x" });
  assert.deepEqual(adapterErrors(f), []);
});

test("a failed `cpd policy show` is an error message and no section", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("policy-show", { ok: false, error: { code: "owner_unset", message: "the Context has no owner", details: {} } }, { exit: 3 });
  const sections = await sectionsAfterStart(f);
  assert.deepEqual(sections, { preamble: "x" });
  const errors = adapterErrors(f);
  assert.equal(errors.length, 1);
  assert.equal(errors[0]!.message.content, "φ adapter: policy show: owner_unset: the Context has no owner");
});

test("the Worker's role: the adapter trusts policy show's role and keeps the domains whose audience holds it", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ role: "worker", plan_lead: false, attempt: "a1" }));
  f.respond("policy-show", policyShow("worker", { deploy: "Lead only.", code: "Small commits.", instructions: "Hi." }, "repo:/p/r.toml"));
  const sections = await sectionsAfterStart(f);
  assert.equal(sections[POLICY_SECTION], "## code (repo:/p/r.toml)\nSmall commits.\n\n## instructions (repo:/p/r.toml)\nHi.");
});

test("a Manager gets only the instructions text", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ role: "manager", plan: null, plan_lead: false }));
  f.respond("policy-show", policyShow("manager", { pr: "Lead and Worker.", instructions: "For all." }));
  const sections = await sectionsAfterStart(f);
  assert.equal(sections[POLICY_SECTION], "## instructions (owner:/p/o.toml)\nFor all.");
});

test("the section composes with the Manager's turn-start update", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ role: "manager", plan: null, plan_lead: false }));
  f.respond("policy-show", policyShow("manager", { instructions: "For all." }));
  f.load();
  await f.pi.fire("session_start");
  const sections: Record<string, string> = {};
  await f.pi.fireWith("before_agent_start", { messageType: "user", prompt: "hi", systemPromptOptions: { sections } });
  assert.ok(sections[POLICY_SECTION]);
  assert.ok(f.calls().some((c) => c.argv[0] === "events" && c.argv[1] === "list"), "the Manager's own handler still ran");
});

test("policySection: an output of another shape is bad_output", () => {
  for (const bad of [{ ok: true }, { ok: true, role: null, policy: {} }, { ok: true, role: "lead", policy: { pr: { text: "x" } } }]) {
    assert.throws(() => policySection(bad), (e: unknown) => e instanceof CpdError && e.code === "bad_output");
  }
});

test("a pi without sections: the policy failure is reported once per run, no turn", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("policy-show", policyShow("lead", { instructions: "Hi." }));
  f.load();
  await f.pi.fire("session_start");
  for (let i = 0; i < 2; i++) await f.pi.fire("before_agent_start", { messageType: "user", prompt: "hi", systemPromptOptions: {} });
  const errs = adapterErrors(f);
  assert.equal(errs.length, 1);
  assert.match(errs[0]!.message.content, /^φ adapter: policy section: sections_missing: /);
});
