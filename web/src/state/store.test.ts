import { describe, expect, it, vi } from "vitest";
import type { ContextNode, ContextTreeModel, PlanNode, SessionNode, InboxModel, PlanViewModel } from "../protocol/cockpit.ts";
import type { Capabilities, Dialog, FooterStats, SessionRef, WireError } from "../protocol/domain.ts";
import type { ServerMessage } from "../protocol/wire.ts";
import { createStore, initialState, reduce, type AppState, type StoreAction } from "./store.ts";

const tree = (cursor: number): ContextTreeModel => ({ contexts: [], cursor });
const plan = { planId: "p1", contextId: "c", title: "P", cursor: 1 } as unknown as PlanViewModel;
const inbox = { openCount: 0, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: [], cursor: 1 } as InboxModel;
const ref = { id: "s1", key: "c:p:j", runner: "pi", role: "worker", state: "working", title: "T" } as SessionRef;
const unknown = { state: "unknown", reason: "n/a" } as const;
const footer = (model: string): FooterStats => ({
  model: { state: "known", value: model }, tier: unknown, thinking: unknown, contextPercent: unknown, cost: unknown, branch: unknown,
});
const caps = {} as Capabilities;
const err: WireError = { code: "host_error", message: "boom" };

const run = (state: AppState, ...actions: StoreAction[]): AppState => actions.reduce(reduce, state);
const server = (message: ServerMessage): StoreAction => ({ type: "server", message });
const attached = (id = "s1", title: string | null = "T"): StoreAction =>
  server({ type: "session.attached", sessionId: id, capabilities: caps, title, meta: { ref, footer: footer("m/1") } });
const selected = (id = "s1"): AppState => run(initialState, { type: "selectSession", sessionId: id });
const pending = (id: string) => ({ clientSubmissionId: id, text: "hi", imageCount: 0, whenBusy: "followUp" as const });

describe("store", () => {
  it("an attach error of the selected session is kept until the same id is selected again", () => {
    const failed = run(selected(), server({ type: "session.error", sessionId: "s1", error: err }));
    expect(failed.session?.error).toEqual(err);
    expect(run(failed, { type: "selectSession", sessionId: "s1" }).session?.error).toBeNull();
  });
  it("cockpit snapshot and patches update the three panes", () => {
    const s1 = run(initialState, server({ type: "cockpit.snapshot", tree: tree(1), plan, inbox }));
    expect(s1.cockpit.tree).toEqual(tree(1));
    expect(s1.cockpit.plan).toEqual(plan);
    expect(s1.cockpit.inbox).toEqual(inbox);
    const s2 = run(s1, server({ type: "cockpit.patch", pane: "tree", data: tree(2) }));
    expect(s2.cockpit.tree).toEqual(tree(2));
    expect(s2.cockpit.plan).toEqual(plan);
    expect(s2.cockpit.inbox).toEqual(inbox);
    const s3 = run(s2, server({ type: "cockpit.patch", pane: "plan", data: null }));
    expect(s3.cockpit.plan).toBeNull();
    expect(s3.cockpit.tree).toEqual(tree(2));
    const s4 = run(s3, server({ type: "cockpit.error", error: err }));
    expect(s4.cockpit.error).toEqual(err);
    expect(s4.cockpit.tree).toEqual(tree(2));
    const s5 = run(s4, server({ type: "cockpit.snapshot", tree: tree(3), plan: null, inbox }));
    expect(s5.cockpit.error).toBeNull();
  });

  it("selecting a session requests attach and detaches the previous one", () => {
    const store = createStore();
    const listener = vi.fn();
    store.subscribe(listener);
    store.dispatch({ type: "selectSession", sessionId: "s1" });
    expect(store.getState().selectedSessionId).toBe("s1");
    expect(store.getState().session).toMatchObject({ id: "s1", attached: false, transcript: { items: [] } });
    expect(listener).toHaveBeenCalledTimes(1);
    store.dispatch({ type: "selectSession", sessionId: "s1" });
    expect(listener).toHaveBeenCalledTimes(1);
    store.dispatch({ type: "server", message: { type: "session.attached", sessionId: "s1", capabilities: caps, title: null, meta: { ref, footer: footer("m/1") } } });
    store.dispatch({ type: "selectSession", sessionId: "s2" });
    expect(store.getState().session?.id).toBe("s2");
    expect(store.getState().session?.attached).toBe(false);
    // a late message of the detached session never lands
    store.dispatch({ type: "server", message: { type: "session.status", sessionId: "s1", busy: true, queued: [] } });
    expect(store.getState().session?.busy).toBe(false);
    store.dispatch({ type: "selectSession", sessionId: null });
    expect(store.getState().session).toBeNull();
    expect(store.getState().selectedSessionId).toBeNull();
  });

  it("submit adds a pending submission that reconciles on ack", () => {
    const s1 = run(selected(), attached(), { type: "submitPending", submission: pending("c1") });
    expect(s1.session?.pending).toEqual([{ ...pending("c1"), state: "sending" }]);
    const s2 = run(s1, server({ type: "session.submitAck", sessionId: "s1", clientSubmissionId: "c1", queued: true }));
    expect(s2.session?.pending).toEqual([]);
    const s3 = run(s1, { type: "dismissPending", clientSubmissionId: "c1" });
    expect(s3.session?.pending).toEqual([]);
  });

  it("failed submit marks the pending submission failed", () => {
    const s1 = run(selected(), attached(), { type: "submitPending", submission: pending("c1") }, { type: "submitPending", submission: pending("c2") });
    const s2 = run(s1, server({ type: "session.error", sessionId: "s1", clientSubmissionId: "c1", error: err }));
    expect(s2.session?.pending).toEqual([
      { ...pending("c1"), state: "failed", error: err },
      { ...pending("c2"), state: "sending" },
    ]);
    expect(s2.session?.error).toBeNull();
    const s3 = run(s2, server({ type: "session.error", sessionId: "s1", error: err }));
    expect(s3.session?.error).toEqual(err);
    const s4 = run(s3, server({ type: "session.error", sessionId: null, error: err }));
    expect(s4.lastError).toEqual(err);
  });

  it("busy state and queued submissions come from session status", () => {
    const queued = [{ clientSubmissionId: "c9", submissionId: 3, text: "later", whenBusy: "followUp" as const }];
    const s1 = run(selected(), attached(), server({ type: "session.status", sessionId: "s1", busy: true, queued }));
    expect(s1.session?.busy).toBe(true);
    expect(s1.session?.queued).toEqual(queued);
    const s2 = run(s1, server({ type: "session.status", sessionId: "s1", busy: false, queued: [] }));
    expect(s2.session?.busy).toBe(false);
    expect(s2.session?.queued).toEqual([]);
  });

  it("active dialog is set and cleared by dialog messages", () => {
    const dialog: Dialog = { id: "d1", interactive: true, method: "cpd_plan_review", title: "T", summary: "S" };
    const s1 = run(selected(), attached(), server({ type: "session.dialog", sessionId: "s1", dialog }));
    expect(s1.session?.dialog).toEqual(dialog);
    const other = run(s1, server({ type: "session.dialogResolved", sessionId: "s1", dialogId: "zzz", outcome: "answered" }));
    expect(other.session?.dialog).toEqual(dialog);
    const s2 = run(s1, server({ type: "session.dialogResolved", sessionId: "s1", dialogId: "d1", outcome: "answered" }));
    expect(s2.session?.dialog).toBeNull();
  });

  it("footer and title come from session meta", () => {
    const s1 = run(selected(), attached("s1", "First"));
    expect(s1.session).toMatchObject({ attached: true, title: "First", ref, footer: footer("m/1"), capabilities: caps });
    const s2 = run(s1, server({ type: "session.footer", sessionId: "s1", footer: footer("m/2") }), server({ type: "session.title", sessionId: "s1", title: "Second" }));
    expect(s2.session?.footer).toEqual(footer("m/2"));
    expect(s2.session?.title).toBe("Second");
  });

  const completion = { label: "safe_to_archive", text: "Safe to archive · 93%", score: 93, tone: "done" } as const;

  it("session.completion sets the completion of the attached session", () => {
    const s1 = run(selected(), attached());
    const s2 = run(s1, server({ type: "session.completion", sessionId: "s1", completion }));
    expect(s2.session?.completion).toEqual(completion);
  });

  it("a null session.completion keeps the shown completion", () => {
    const s1 = run(selected(), attached(), server({ type: "session.completion", sessionId: "s1", completion }));
    const s2 = run(s1, server({ type: "session.completion", sessionId: "s1", completion: null }));
    expect(s2.session?.completion).toEqual(completion);
  });

  it("selecting a session shows its last known completion at once", () => {
    const s1 = run(selected(), attached(), server({ type: "session.completion", sessionId: "s1", completion }));
    const s2 = run(s1, { type: "selectSession", sessionId: "s2" });
    expect(s2.session?.completion).toBeNull();
    const back = run(s2, { type: "selectSession", sessionId: "s1" });
    expect(back.session?.completion).toEqual(completion);
    expect(run(back, { type: "selectSession", sessionId: "never-seen" }).session?.completion).toBeNull();
  });

  it("session.completion for another session is ignored", () => {
    const s1 = run(selected(), attached(), server({ type: "session.completion", sessionId: "s1", completion }));
    const other = { ...completion, label: "open_work", text: "Open work · 40%", score: 40, tone: "decision" } as const;
    const s2 = run(s1, server({ type: "session.completion", sessionId: "other", completion: other }));
    expect(s2.session?.completion).toEqual(completion);
  });

  it("a new session starts with completion null", () => {
    expect(selected().session?.completion).toBeNull();
    const s1 = run(selected(), attached(), server({ type: "session.completion", sessionId: "s1", completion }));
    expect(s1.session?.completion).toEqual(completion);
    expect(run(s1, { type: "selectSession", sessionId: "s2" }).session?.completion).toBeNull();
  });

  const ack = (id: string, queued: boolean): StoreAction =>
    server({ type: "session.submitAck", sessionId: "s1", clientSubmissionId: id, queued });
  const userItem = (id: string, text: string) => ({ kind: "user" as const, id, text, attachments: [] });
  const sentState = (...texts: string[]): AppState => {
    const submits = texts.map((text, i): StoreAction => ({ type: "submitPending", submission: { ...pending(`c${i + 1}`), text } }));
    const acks = texts.map((_, i) => ack(`c${i + 1}`, false));
    return run(selected(), attached(), ...submits, ...acks);
  };

  it("submitAck of a non-queued submission keeps the pending row as sent", () => {
    const s = run(selected(), attached(), { type: "submitPending", submission: pending("c1") }, ack("c1", false));
    expect(s.session?.pending).toEqual([{ ...pending("c1"), state: "sent" }]);
  });

  it("submitAck of a queued submission removes the pending row", () => {
    const s = run(
      selected(), attached(),
      { type: "submitPending", submission: pending("c1") }, { type: "submitPending", submission: pending("c2") },
      ack("c1", true), ack("c2", false),
    );
    expect(s.session?.pending).toEqual([{ ...pending("c2"), state: "sent" }]);
  });

  it("a new user item with the same text removes the sent pending row", () => {
    const s1 = sentState("hi", "other", "hi");
    expect(s1.session?.pending.map((p) => p.state)).toEqual(["sent", "sent", "sent"]);
    const s2 = run(s1, server({ type: "session.append", sessionId: "s1", items: [userItem("u:1", "  hi \n")] }));
    expect(s2.session?.pending.map((p) => p.clientSubmissionId)).toEqual(["c2", "c3"]);
    const s3 = run(s2, server({ type: "session.update", sessionId: "s1", items: [userItem("u:1", "hi")] }));
    expect(s3.session?.pending.map((p) => p.clientSubmissionId)).toEqual(["c2", "c3"]);
    const s4 = run(s3, server({ type: "session.append", sessionId: "s1", items: [userItem("u:2", "hi")] }));
    expect(s4.session?.pending.map((p) => p.clientSubmissionId)).toEqual(["c2"]);
  });

  it("a new user item with different text keeps the sent pending row", () => {
    const s1 = sentState("hi");
    expect(s1.session?.pending[0]?.state).toBe("sent");
    const s2 = run(s1, server({ type: "session.append", sessionId: "s1", items: [userItem("u:1", "different")] }));
    expect(s2.session?.pending).toEqual([{ ...pending("c1"), state: "sent" }]);
  });

  it("a tail window clears sent pending rows", () => {
    const s1 = run(sentState("hi", "yo"), { type: "submitPending", submission: pending("c3") });
    expect(s1.session?.pending.map((p) => p.state)).toEqual(["sent", "sent", "sending"]);
    const s2 = run(s1, server({ type: "session.window", sessionId: "s1", mode: "tail", window: { items: [], hasEarlier: false, cursor: null } }));
    expect(s2.session?.pending).toEqual([{ ...pending("c3"), state: "sending" }]);
  });

  it("serverBatch applies messages in order and notifies once", () => {
    const store = createStore(run(selected(), attached()));
    const listener = vi.fn();
    store.subscribe(listener);
    store.dispatch({
      type: "serverBatch",
      messages: [
        { type: "session.append", sessionId: "s1", items: [userItem("u:1", "a")] },
        { type: "session.append", sessionId: "s1", items: [userItem("u:2", "b")] },
        { type: "session.update", sessionId: "s1", items: [userItem("u:1", "A")] },
      ],
    });
    expect(listener).toHaveBeenCalledTimes(1);
    expect(store.getState().session?.transcript.items.map((i) => [i.id, (i as { text: string }).text])).toEqual([["u:1", "A"], ["u:2", "b"]]);
  });
});

describe("optimistic merge", () => {
  const prRow = (id: number, subject: string, kind = "pr-ready"): InboxModel["rows"][number] => ({
    id, kind, label: "ready to merge", urgency: "normal", contextId: null, planId: "p1", jobKey: null, sessionId: null,
    url: null, subject, workflow: null, openedAt: "t", ageSeconds: 0, actions: [],
  } as unknown as InboxModel["rows"][number]);
  const rows = [prRow(1, "pr:acme/api#3"), prRow(2, "pr:acme/api#4"), prRow(3, "pr:acme/api#3", "review-requested")];
  const withRows = { ...inbox, rows, openCount: 3 } as InboxModel;
  const loaded = (): AppState => run(initialState, server({ type: "cockpit.snapshot", tree: tree(1), plan, inbox: withRows }));
  const merge: StoreAction = { type: "actionOptimistic", actionId: "a1", action: { kind: "pr.merge", planId: "p1", pull: "acme/api#3" } };
  const ids = (s: AppState): number[] => (s.cockpit.inbox?.rows ?? []).map((r) => r.id);

  it("merge_action_removes_pr_row_optimistically", () => {
    expect(ids(run(loaded(), merge))).toEqual([2, 3]);
  });

  it("merge_failure_rolls_back_row", () => {
    const removed = run(loaded(), merge);
    expect(ids(removed)).toEqual([2, 3]);
    const failed = run(removed, server({ type: "action.result", actionId: "a1", ok: false, error: err }));
    expect(ids(failed)).toEqual([1, 2, 3]);
  });

  it("merge_success_keeps_row_removed_until_refresh", () => {
    const removed = run(loaded(), merge);
    const ok = run(removed, server({ type: "action.result", actionId: "a1", ok: true, output: {} }));
    expect(ids(ok)).toEqual([2, 3]);
    const fresh = { ...withRows, rows: [rows[1]!], openCount: 1 } as InboxModel;
    const refreshed = run(ok, server({ type: "cockpit.patch", pane: "inbox", data: fresh }));
    expect(refreshed.cockpit.inbox).toEqual(fresh);
  });
});

describe("optimistic merge race", () => {
  const prRow = (id: number, subject: string): InboxModel["rows"][number] => ({
    id, kind: "pr-ready", label: "ready to merge", urgency: "normal", contextId: null, planId: "p1", jobKey: null, sessionId: null,
    url: null, subject, workflow: null, openedAt: "t", ageSeconds: 0, actions: [],
  } as unknown as InboxModel["rows"][number]);
  const rows = [prRow(1, "pr:acme/api#3"), prRow(2, "pr:acme/api#4")];
  const at = (cursor: number, list: InboxModel["rows"]): InboxModel => ({ ...inbox, rows: list, openCount: list.length, cursor });
  const loaded = (): AppState => run(initialState, server({ type: "cockpit.snapshot", tree: tree(1), plan, inbox: at(1, rows) }));
  const merge = (actionId: string): StoreAction => ({ type: "actionOptimistic", actionId, action: { kind: "pr.merge", planId: "p1", pull: "acme/api#3" } });
  const patch = (data: InboxModel): StoreAction => server({ type: "cockpit.patch", pane: "inbox", data });
  const ids = (s: AppState): number[] => (s.cockpit.inbox?.rows ?? []).map((r) => r.id);
  const changed = (n: number): InboxModel["rows"] => [rows[0]!, { ...rows[1]!, label: `l${n}` }];

  it("patch_keeps_optimistic_merge_row_hidden", () => {
    let s = run(loaded(), merge("a1"));
    for (const n of [2, 3]) {
      s = run(s, patch(at(n, changed(n))));
      expect(ids(s)).toEqual([2]);
      expect(s.cockpit.inbox?.rows[0]?.label).toBe(`l${n}`);
    }
  });

  it("snapshot_keeps_optimistic_merge_row_hidden", () => {
    const s = run(loaded(), merge("a1"), server({ type: "cockpit.snapshot", tree: tree(2), plan, inbox: at(2, changed(2)) }));
    expect(ids(s)).toEqual([2]);
    expect(s.cockpit.inbox?.rows[0]?.label).toBe("l2");
  });

  it("duplicate_merge_failure_keeps_row_gone", () => {
    const stale = at(1, rows);
    const gone = at(2, [rows[1]!]); // a later inbox patch removed the merged row
    const entry = { before: stale, after: gone, subject: "pr:acme/api#3" };
    const optimistic = { c1: entry, c2: entry } as unknown as AppState["optimistic"];
    const base: AppState = { ...loaded(), optimistic, pendingActionIds: ["c1", "c2"], cockpit: { ...loaded().cockpit, inbox: gone } };
    const failed = run(base, server({ type: "action.result", actionId: "c2", ok: false, error: err }));
    expect(ids(failed)).toEqual([2]);
  });

  it("stale_inbox_patch_is_ignored", () => {
    const s = run(loaded(), patch(at(5, [rows[1]!])));
    const after = run(s, patch(at(3, rows)));
    expect(after.cockpit.inbox).toEqual(s.cockpit.inbox);
  });
});

describe("archive navigation", () => {
  const counts = { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 };
  const sess = (id: string, role: "manager" | "lead", contextId: string, planId: string | null, archived = false): SessionNode => ({
    ref: { id, key: planId === null ? contextId : `${contextId}:${planId}`, runner: "pi", role, state: "idle", title: null },
    contextId, planId, jobKey: null, archived, dot: { state: "idle", attention: null },
  });
  const planNode = (contextId: string, id: string, archived = false): PlanNode => ({
    id, contextId, title: id, phase: "dispatch", archived, counts, leadSessionId: `lead-${id}`,
    sessions: [sess(`lead-${id}`, "lead", contextId, id, archived)],
  });
  const ctx = (id: string, planIds: string[], archived = false, archivedPlans: string[] = []): ContextNode => ({
    id, title: id, archived, records: 0, openQuestions: 0,
    sessions: [sess(`mgr-${id}`, "manager", id, null, archived)],
    plans: planIds.map((p) => planNode(id, p, archived || archivedPlans.includes(p))),
  });
  const model = (contexts: ContextNode[]): ContextTreeModel => ({ contexts, cursor: 1 });
  const snap = (t: ContextTreeModel): StoreAction => server({ type: "cockpit.snapshot", tree: t, plan: null, inbox });
  const patch = (t: ContextTreeModel): StoreAction => server({ type: "cockpit.patch", pane: "tree", data: t });
  const at = (t: ContextTreeModel, sessionId: string, planId: string | null): AppState =>
    run(initialState, snap(t), { type: "selectPlan", planId }, { type: "selectSession", sessionId });
  const where = (s: AppState) => [s.selectedSessionId, s.cockpit.selectedPlanId];

  it("tree update archiving the selected Lead selects the next live Plan Lead of the same Context", () => {
    const live = model([ctx("cpd", []), ctx("a", ["p1", "p2"])]);
    const after = model([ctx("cpd", []), ctx("a", ["p1", "p2"], false, ["p1"])]);
    expect(where(run(at(live, "lead-p1", "p1"), patch(after)))).toEqual(["lead-p2", "p2"]);
  });

  it("tree update archiving the last live Plan selects the Context Manager with planId null", () => {
    const live = model([ctx("cpd", []), ctx("a", ["p1"])]);
    const after = model([ctx("cpd", []), ctx("a", ["p1"], false, ["p1"])]);
    expect(where(run(at(live, "lead-p1", "p1"), patch(after)))).toEqual(["mgr-a", null]);
  });

  it("tree update archiving the selected Context selects the next live non-standing Context Manager", () => {
    const live = model([ctx("cpd", []), ctx("a", []), ctx("b", [])]);
    const after = model([ctx("cpd", []), ctx("a", [], true), ctx("b", [])]);
    expect(where(run(at(live, "mgr-a", null), patch(after)))).toEqual(["mgr-b", null]);
  });

  it("tree update archiving the last non-standing Context selects the cpd Manager", () => {
    const live = model([ctx("cpd", []), ctx("host", []), ctx("a", [])]);
    const after = model([ctx("cpd", []), ctx("host", []), ctx("a", [], true)]);
    expect(where(run(at(live, "mgr-a", null), patch(after)))).toEqual(["mgr-cpd", null]);
  });

  it("opening an archived row keeps the selection", () => {
    const live = model([ctx("cpd", []), ctx("a", ["p1", "p2"])]);
    const archived = model([ctx("cpd", []), ctx("a", ["p1", "p2"], false, ["p1"])]);
    const moved = run(at(live, "lead-p1", "p1"), patch(archived));
    expect(where(moved)).toEqual(["lead-p2", "p2"]); // the live-to-archived transition navigated
    const opened = run(moved, { type: "selectPlan", planId: "p1" }, { type: "selectSession", sessionId: "lead-p1" });
    const later = run(opened, patch({ ...archived, cursor: 2 }));
    expect(where(later)).toEqual(["lead-p1", "p1"]);
  });

  it("tree update that leaves the selected session live keeps the selection", () => {
    const live = model([ctx("cpd", []), ctx("a", ["p1", "p2"])]);
    const other = model([ctx("cpd", []), ctx("a", ["p1", "p2"], false, ["p2"])]);
    const kept = run(at(live, "lead-p1", "p1"), patch(other));
    expect(where(kept)).toEqual(["lead-p1", "p1"]);
    const archiveSelected = model([ctx("cpd", []), ctx("a", ["p1", "p2"], false, ["p1", "p2"])]);
    expect(where(run(kept, patch(archiveSelected)))).toEqual(["mgr-a", null]); // only a transition of the selected one navigates
  });
});
