import assert from "node:assert/strict";
import { test } from "node:test";
import { hostDeps, type HostModels } from "../../durable-host/main.ts";
import type { Config } from "../../durable-host/config.ts";

const CONFIG: Config = { home: "/cpd-home", session: "s-lead", cwd: "/work/tree", model: "anthropic/sonnet:high" };

/** A fake model runtime: records the classifier lookups and answers what the test says. No network, no model. */
function fakeModels(reply: unknown) {
  const lookups: unknown[][] = [];
  const classified: { model: unknown; context: { state?: { message?: string } } }[] = [];
  const models: HostModels = {
    getAvailable: async () => [],
    getModelOfType: (...args: unknown[]) => {
      lookups.push(args);
      return { id: "kev-4b" };
    },
    classify: async (model: unknown, context: unknown) => {
      classified.push({ model, context: context as never });
      return reply;
    },
  } as never;
  return { models, lookups, classified };
}

test("the host deps give the question guard a classifier", async () => {
  const { models, lookups, classified } = fakeModels({ stopReason: "stop", answers: { asking_user: { type: "bool", probability: 0.93 } } });
  const deps = hostDeps(CONFIG, models);
  assert.equal(typeof deps.classify, "function", "Deps.classify is wired");
  assert.equal(await deps.classify!("Shall I proceed with the migration?"), 0.93);
  assert.deepEqual(lookups, [["classifier", "llama.cpp", "kev-4b"]]);
  assert.equal(classified.length, 1);
  assert.match(String(classified[0]!.context.state?.message), /Shall I proceed/);

  const down = hostDeps(CONFIG, fakeModels({ stopReason: "error", errorMessage: "fetch failed" }).models);
  assert.equal(await down.classify!("Shall I proceed?"), undefined, "an unreachable classifier answers undefined");
});
