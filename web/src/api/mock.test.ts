// Mock-contract tests (docs/research/web-client-api.md §4). RED: `createMockApi()` still
// throws `not implemented`, so they fail now and pass once the impl lands.
import { expect, test } from "vitest";
import { createMockApi } from "./mock";
import type { UiResponse } from "./types";

test("the mock refuses a request without the token", () => {
  const mock = createMockApi();
  // A token-carrying request is accepted (§4.2, hs-d781).
  expect(() =>
    mock.submit({ token: "test-token", type: "submit", req: "r1", text: "hello", when_busy: "followUp" }),
  ).not.toThrow();
  // A request without the token is refused.
  expect(() => mock.submit({ type: "submit", req: "r2", text: "hello", when_busy: "followUp" })).toThrow(/token/i);
});

test("ui_response first answer wins and a late one is ui_unknown", () => {
  const mock = createMockApi();
  const first: UiResponse = { type: "ui_response", req: "a", id: "p1", value: "yes" };
  const late: UiResponse = { type: "ui_response", req: "b", id: "p1", confirmed: true };
  // The first answer is accepted and answers the sender's req.
  const answered = mock.uiResponse(first);
  expect(answered.req).toBe("a");
  // A late answer on the same held id is `ui_unknown` (§4.3, durable-leads 4.4 rule 3).
  expect(() => mock.uiResponse(late)).toThrow(/ui_unknown/i);
});
