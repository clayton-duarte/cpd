// Mock-contract tests (docs/research/web-client-api.md §4). RED: `createMockApi()` still
// throws `not implemented`, so they fail now and pass once the impl lands.
import { expect, test } from "vitest";
import { createMockApi } from "./mock";
import type { UiResponse } from "./types";

test("the mock refuses a request without the token", () => {
  const mock = createMockApi();
  // A token-carrying request is accepted.
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
  // The first answer is accepted (it resolves the held request).
  expect(() => mock.uiResponse(first)).not.toThrow();
  // A late answer on the same held id is `ui_unknown`.
  expect(() => mock.uiResponse(late)).toThrow(/ui_unknown/i);
});
