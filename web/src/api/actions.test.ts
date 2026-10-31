// Typed-POST action tests (docs/research/web-client-api.md §4.4). RED: `mock.post` still
// throws `not implemented`, so they fail now and pass once the impl lands. The builders are
// real data constructors — the mock's action handlers are the stubs.
import { expect, test } from "vitest";
import { createMockApi } from "./mock";
import { submitPost, uiResponsePost } from "./actions";

test("actions are typed POSTs carried to the mock", () => {
  const mock = createMockApi();
  const submit = submitPost("hello", { when_busy: "steer" });
  // The builder makes a typed POST.
  expect(submit.action).toBe("submit");
  expect(submit.payload.text).toBe("hello");
  expect(submit.payload.when_busy).toBe("steer");
  // A typed POST is carried to the mock with the token.
  expect(() => mock.post(submit, { token: "test-token" })).not.toThrow();

  const answer = uiResponsePost({ type: "ui_response", req: "a", id: "p1", value: "yes" });
  expect(answer.action).toBe("ui_response");
  expect(() => mock.post(answer, { token: "test-token" })).not.toThrow();
});
