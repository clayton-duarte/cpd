import { describe, expect, it } from "vitest";
import type { Capabilities, FooterStats, SessionRef, ToolItem, WireError } from "../protocol/domain.ts";
import type { ServerMessage } from "../protocol/wire.ts";
import { initialState, reduce, type AppState, type StoreAction } from "./store.ts";

const ref = { id: "s1", key: "c:p:j", runner: "pi", role: "worker", state: "working", title: "T" } as SessionRef;
const unknown = { state: "unknown", reason: "n/a" } as const;
const footer: FooterStats = { model: unknown, tier: unknown, thinking: unknown, contextPercent: unknown, cost: unknown, branch: unknown };
const server = (message: ServerMessage): StoreAction => ({ type: "server", message });
const run = (state: AppState, ...actions: StoreAction[]): AppState => actions.reduce(reduce, state);
const attached = (): AppState => run(initialState,
  { type: "selectSession", sessionId: "s1" },
  server({ type: "session.attached", sessionId: "s1", capabilities: {} as Capabilities, title: "T", meta: { ref, footer } }),
);
const success: ToolItem = {
  kind: "tool", id: "tool:success", callId: "success", name: "bash", args: { command: "cpd session send ..." },
  status: "done", result: { text: "Message sent successfully", isError: false },
};
const running: ToolItem = {
  kind: "tool", id: "tool:compound", callId: "compound", name: "bash",
  args: { command: "cpd session send ...; cpd job show bottom-inset-fix --plan desktop-shell" }, status: "running",
};
const failed: ToolItem = {
  ...running, status: "failed", result: { text: "Message sent successfully\nplan_has_lead\nCommand exited with code 3", isError: true },
};
const tail = (items: ToolItem[]): StoreAction => server({
  type: "session.window", sessionId: "s1", mode: "tail", window: { items, hasEarlier: false, cursor: null },
});
const expectNoToast = (state: AppState, items: ToolItem[]): void => {
  expect(state.session?.transcript.items).toEqual(items);
  expect(state.lastError ?? state.cockpit.error).toBeNull();
};

describe("agent failures and action toasts", () => {
  it("retains a compound failure with successful stdout without a toast on live update", () => {
    const before = run(attached(), tail([success, running]));
    expectNoToast(before, [success, running]);
    const after = run(before, server({ type: "session.update", sessionId: "s1", items: [failed] }));
    expectNoToast(after, [success, failed]);
  });

  it("retains historical failures without a toast on reconnect replay", () => {
    const historical = run(attached(), tail([success, failed]));
    expect(historical.session?.transcript.items).toEqual([success, failed]);
    // Dismiss an existing toast before reconnect, so replay must not raise a new one.
    const reconnected = run(historical, { type: "dismissError" }, { type: "connection", status: "reconnecting" },
      { type: "connection", status: "open" }, tail([success, failed]));
    expectNoToast(reconnected, [success, failed]);
    expectNoToast(historical, [success, failed]);
  });

  it("still shows a user action failure after an agent failure", () => {
    const actionError: WireError = { code: "cpd_failed", message: "action failed" };
    const state = run(attached(), tail([success, failed]),
      server({ type: "action.result", actionId: "a1", ok: false, error: actionError }));
    expect(state.session?.transcript.items).toEqual([success, failed]);
    expect(state.lastError ?? state.cockpit.error).toEqual(actionError);
  });
});
