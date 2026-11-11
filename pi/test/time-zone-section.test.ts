import assert from "node:assert/strict";
import { test } from "node:test";
import { TIME_ZONE_SECTION } from "../lib/time.ts";
import { Fixture, policyShow, shown } from "./fake.ts";

async function sectionsAfterStart(f: Fixture): Promise<Record<string, string>> {
  f.load();
  await f.pi.fire("session_start");
  const sections: Record<string, string> = { preamble: "x" };
  await f.pi.fire("before_agent_start", { messageType: "user", prompt: "hi", systemPromptOptions: { sections } });
  return sections;
}

test("every role gets the time-zone section", async (t) => {
  const shows = {
    manager: shown({ role: "manager", plan: null, plan_lead: false }),
    lead: shown({ role: "lead" }),
    worker: shown({ role: "worker", plan_lead: false, attempt: "a1" }),
  };
  for (const role of ["manager", "lead", "worker"] as const) {
    const f = new Fixture();
    t.after(() => f.close());
    f.respond("session-show", shows[role]);
    f.respond("policy-show", policyShow(role, { instructions: "Hi." }));
    const section = (await sectionsAfterStart(f))[TIME_ZONE_SECTION];
    assert.ok(section, `${role} has the section`);
    assert.ok(section.startsWith("Time zone: "), role);
  }
});

test("the section is set when there is no policy", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const section = (await sectionsAfterStart(f))[TIME_ZONE_SECTION];
  assert.ok(section);
  assert.ok(section.startsWith("Time zone: "));
});

test("two consecutive turns in different minutes produce identical time-zone sections", async (t) => {
  t.mock.timers.enable({ apis: ["Date"] });
  t.mock.timers.setTime(Date.parse("2026-10-25T22:29:00.000Z"));
  const f = new Fixture();
  t.after(() => f.close());
  f.load();
  await f.pi.fire("session_start");
  const sections1: Record<string, string> = { preamble: "x" };
  await f.pi.fire("before_agent_start", { messageType: "user", prompt: "turn 1", systemPromptOptions: { sections: sections1 } });

  t.mock.timers.setTime(Date.parse("2026-10-25T22:31:00.000Z"));
  const sections2: Record<string, string> = { preamble: "x" };
  await f.pi.fire("before_agent_start", { messageType: "user", prompt: "turn 2", systemPromptOptions: { sections: sections2 } });

  assert.equal(sections1[TIME_ZONE_SECTION], sections2[TIME_ZONE_SECTION]);
});

