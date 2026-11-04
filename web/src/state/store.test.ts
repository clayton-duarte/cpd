import { describe, expect, it, vi } from "vitest";
import type { ContextTreeModel, InboxModel, PlanViewModel } from "../protocol/cockpit.ts";
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
