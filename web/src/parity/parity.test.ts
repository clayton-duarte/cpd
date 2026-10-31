// Parity skeleton (docs/research/web-client-api.md §3, hs-d771/hs-d779): the read-model
// mirror accepts the pinned CLI JSON fixture. RED: `acceptReadModel` still throws
// `not implemented`, so this fails now and passes once the impl lands (and, when W1 lands,
// the same guard asserts the core `Serialize` output equals the mirror).
import { expect, test } from "vitest";
import { acceptReadModel, type PlanListResult } from "../api/readmodel";
import fixture from "./fixture.json";

test("the read-model mirror accepts the pinned CLI JSON fixture", () => {
  const parsed = acceptReadModel<PlanListResult>(fixture);
  expect(parsed.ok).toBe(true);
  expect(parsed.plans.length).toBeGreaterThan(0);
  expect(parsed.plans[0]?.id).toBe("plan-1");
});
