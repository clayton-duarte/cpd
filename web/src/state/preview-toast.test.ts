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
  const guardRunning: ToolItem = {
    kind: "tool", id: "tool:guard", callId: "guard", name: "bash",
    args: { command: "cpd job show example", timeout: 60 }, status: "running",
  };
  // Representative guard output; the source session of the reported toast is unverified.
  const guardRefusal: ToolItem = {
    ...guardRunning, status: "failed", result: {
      text: "φ: a Lead never blocks: a wait over 30 s runs in the background or is left to the engine's Events; refused: bash timeout 60",
      isError: true,
    },
  };
  const ordinaryFailure: ToolItem = {
    kind: "tool", id: "tool:ordinary", callId: "ordinary", name: "bash", args: { command: "false" },
    status: "failed", result: { text: "Command exited with code 1", isError: true },
  };
  const helpFailure: ToolItem = {
    kind: "tool", id: "tool:help", callId: "help", name: "bash",
    args: { command: "cpd job edit --help; false" }, status: "failed",
    result: { text: '{"ok":true,"help":"cpd job edit"}\nCommand exited with code 1', isError: true },
  };

  // Payloads reported in the incident, not claims about the originating calls.
  const reportedFailures: ToolItem[] = [
    { kind: "tool", id: "tool:script", callId: "script", name: "codemode", args: {},
      status: "failed", result: { text: "Script failed", isError: true } },
    { kind: "tool", id: "tool:explore", callId: "explore", name: "bash", args: {},
      status: "failed", result: { text: "φ: explore artefact: $CPD_HOME/run/<plan>/explore is for Workers; read the job-merged summary or add a follow-up scout Job", isError: true } },
    { kind: "tool", id: "tool:spec", callId: "spec", name: "bash", args: {},
      status: "failed", result: { text: "spec_invalid: engine-defects /tmp/cpd-engine-defects/followup-cleanup.json $.touches", isError: true } },
    { kind: "tool", id: "tool:tier", callId: "tier", name: "bash", args: {},
      status: "failed", result: { text: "tier_not_allowed: worker SX rejected; allowed HX-S0, floor HX, ceiling S0, key worker_tier; next-role lead_tier ceiling", isError: true } },
    // air-n209 reports the payload, not the calling tool or arguments; these are fixture placeholders.
    { kind: "tool", id: "tool:host-not-found", callId: "host-not-found", name: "unknown", args: {},
      status: "failed", result: { text: JSON.stringify({ code: "session_not_found", message: "No Session cpd:host", details: { session: "cpd:host" } }), isError: true } },
  ];

  it.each(reportedFailures)("keeps reported $id failures in the transcript without a toast", (failure) => {
    const running: ToolItem = { ...failure, status: "running", result: undefined };
    const before = run(attached(), tail([running]));
    const updated = run(before, server({ type: "session.update", sessionId: "s1", items: [failure] }));
    const appended = run(attached(), server({ type: "session.append", sessionId: "s1", items: [failure] }));
    expectNoToast(appended, [failure]);
    expectNoToast(updated, [failure]);
    const replayed = run(updated, { type: "connection", status: "reconnecting" },
      { type: "connection", status: "open" }, tail([failure]));
    expectNoToast(replayed, [failure]);
    const actionError: WireError = { code: "cpd_failed", message: "action failed" };
    const actionFirst = run(before, server({ type: "action.result", actionId: "a1", ok: false, error: actionError }),
      server({ type: "session.update", sessionId: "s1", items: [failure] }));
    expect(actionFirst.session?.transcript.items).toEqual([failure]);
    expect(actionFirst.lastError ?? actionFirst.cockpit.error).toEqual(actionError);
  });

  it("keeps a live Lead guard refusal as a failed row, not a toast", () => {
    const before = run(attached(), tail([guardRunning]));
    expectNoToast(before, [guardRunning]);
    const after = run(before, server({ type: "session.update", sessionId: "s1", items: [guardRefusal] }));
    expectNoToast(after, [guardRefusal]);
  });

  it("keeps ordinary failures and successful help stdout from a failed compound command in the transcript", () => {
    const after = run(attached(), server({ type: "session.append", sessionId: "s1", items: [ordinaryFailure, helpFailure] }));
    expectNoToast(after, [ordinaryFailure, helpFailure]);
  });

  it("replays guard and ordinary failures without a toast on attach and reconnect", () => {
    const items = [guardRefusal, ordinaryFailure, helpFailure];
    const historical = run(attached(), tail(items));
    expectNoToast(historical, items);
    const reconnected = run(historical, { type: "connection", status: "reconnecting" },
      { type: "connection", status: "open" }, tail(items));
    expectNoToast(reconnected, items);
  });

  it("keeps an existing user-action toast when an agent guard refusal arrives", () => {
    const actionError: WireError = { code: "cpd_failed", message: "action failed" };
    const before = run(attached(), tail([guardRunning]),
      server({ type: "action.result", actionId: "a1", ok: false, error: actionError }));
    const after = run(before, server({ type: "session.update", sessionId: "s1", items: [guardRefusal] }));
    expect(after.session?.transcript.items).toEqual([guardRefusal]);
    expect(after.lastError ?? after.cockpit.error).toEqual(actionError);
  });
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
