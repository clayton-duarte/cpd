import { expect, it } from "vitest";
import type { WireError } from "../protocol/domain.ts";
import type { ServerMessage } from "../protocol/wire.ts";
import { initialState, reduce, type AppState, type StoreAction } from "./store.ts";

// `pendingActionIds`, `completedActionIds` and the `actionStart` action are the pro-d53 contract
// (CONTRACT 11.2, DESIGN-panes 2.7); they are read and sent through these casts until the store has them.
interface ActionIds {
readonly pendingActionIds: readonly string[];
readonly completedActionIds: readonly string[];
}
const ids = (s: AppState): ActionIds => s as unknown as ActionIds;
const start = (actionId: string): StoreAction => ({ type: "actionStart", actionId }) as unknown as StoreAction;
const result = (message: ServerMessage): StoreAction => ({ type: "server", message });
const err: WireError = { code: "cpd_failed", message: "merge failed" };
const run = (state: AppState, ...actions: StoreAction[]): AppState => actions.reduce(reduce, state);

it("store tracks pendingActionIds until the action result arrives", () => {
  expect(ids(initialState).pendingActionIds).toEqual([]);
  expect(ids(initialState).completedActionIds).toEqual([]);
  const s1 = run(initialState, start("a1"), start("a2"), start("a1"));
  expect(ids(s1).pendingActionIds).toEqual(["a1", "a2"]);
  const failed = run(s1, result({ type: "action.result", actionId: "a1", ok: false, error: err }));
  expect(ids(failed).pendingActionIds).toEqual(["a2"]);
  expect(ids(failed).completedActionIds).toEqual([]);
  expect(failed.lastError).toEqual(err);
});

it("store records completedActionIds on an ok action result", () => {
  const s1 = run(initialState, start("a1"), start("a2"));
  const ok = run(s1, result({ type: "action.result", actionId: "a1", ok: true, output: {} }));
  expect(ids(ok).pendingActionIds).toEqual(["a2"]);
  expect(ids(ok).completedActionIds).toEqual(["a1"]);
  expect(ok.lastError).toBeNull();
  const again = run(ok, result({ type: "action.result", actionId: "a1", ok: true, output: {} }));
  expect(ids(again).completedActionIds).toEqual(["a1"]);
});
