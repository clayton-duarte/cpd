import { describe, expect, it } from "vitest";
import type { Disposable } from "../../protocol/runtime.ts";
import type { ServerMessage } from "../../protocol/wire.ts";
import { WebError } from "../../protocol/domain.ts";
import { createHubConnection, type HubDeps } from "./connection.ts";

type Handle = Awaited<ReturnType<HubDeps["backends"]["pi"]["attach"]>>;
type Emit = Parameters<HubDeps["backends"]["pi"]["attach"]>[1];
type Resolved = ReturnType<HubDeps["registry"]["resolve"]>;
type Listener = Parameters<HubDeps["cockpit"]["subscribe"]>[0];

const caps = {} as Handle["capabilities"];

const flush = async (): Promise<void> => {
  for (let i = 0; i < 20; i++) await Promise.resolve();
};

class FakeHandle {
  calls: Array<[string, ...unknown[]]> = [];
  disposed = 0;
  failWith: unknown = null;
  readonly capabilities = caps;
  private run(name: string, ...args: unknown[]): Promise<void> {
    this.calls.push([name, ...args]);
    return this.failWith === null ? Promise.resolve() : Promise.reject(this.failWith);
  }
  loadEarlier(b: string) { return this.run("loadEarlier", b); }
  submit(i: unknown) { return this.run("submit", i); }
  abort() { return this.run("abort"); }
  abortSubmission(id: string) { return this.run("abortSubmission", id); }
  compact(i?: string) { return this.run("compact", i); }
  configure(c: unknown) { return this.run("configure", c); }
  listModels() { return this.run("listModels"); }
  respond(id: string, r: unknown) { return this.run("respond", id, r); }
  dispose() { this.disposed += 1; return Promise.resolve(); }
}

class FakeBackend {
  attaches: Array<{ session: Resolved; emit: Emit; handle: FakeHandle }> = [];
  attach(session: Resolved, emit: Emit): Promise<Handle> {
    const handle = new FakeHandle();
    this.attaches.push({ session, emit, handle });
    return Promise.resolve(handle);
  }
}

const refOf = (id: string, runner: "pi" | "durable") => ({ id, key: id, runner, role: "worker", state: "idle", title: null });

const setup = (opts: { ready?: Promise<void> } = {}) => {
  const sent: ServerMessage[] = [];
  const listeners: Listener[] = [];
  let subscriptions = 0;
  let disposedSubs = 0;
  const tree = { contexts: [], cursor: 1 };
  const inbox = { openCount: 0, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: [], cursor: 1 };
  let snap: ReturnType<HubDeps["cockpit"]["snapshot"]> = { tree, plan: null, inbox } as never;
  const selected: Array<string | null> = [];
  const actions: unknown[] = [];
  let actionResult: () => Promise<unknown> = () => Promise.resolve({ stdout: "ok" });
  const cockpit = {
    snapshot: () => snap,
    // the cockpit service's readiness: it resolves when the first `readModel.loadAll` has landed
    ready: () => opts.ready ?? Promise.resolve(),
    selectPlan: (p: string | null) => { selected.push(p); return Promise.resolve(); },
    subscribe: (l: Listener): Disposable => {
      subscriptions += 1;
      listeners.push(l);
      let done = false;
      return { dispose: () => { if (!done) { done = true; disposedSubs += 1; } } };
    },
    runAction: (a: unknown) => { actions.push(a); return actionResult(); },
  } as unknown as HubDeps["cockpit"];
  const registry: HubDeps["registry"] = {
    resolve: (id: string) => {
      if (id === "pi-1") return { ref: refOf(id, "pi"), file: "/f.jsonl", tierOptions: [] } as unknown as Resolved;
      if (id === "pi-2") return { ref: refOf(id, "pi"), file: "/g.jsonl", tierOptions: [] } as unknown as Resolved;
      if (id === "lead-1") return { ref: { ...refOf(id, "pi"), role: "lead" }, file: "/l.jsonl", tierOptions: [] } as unknown as Resolved;
      if (id === "mgr-1") return { ref: { ...refOf(id, "pi"), role: "manager" }, file: "/m.jsonl", tierOptions: [] } as unknown as Resolved;
      if (id === "dur-1") return { ref: refOf(id, "durable"), socketPath: "/s.sock", tierOptions: [] } as unknown as Resolved;
      throw new WebError("session_not_found", "no such session", { sessionId: id });
    },
  };
  const pi = new FakeBackend();
  const durable = new FakeBackend();
  const hub = createHubConnection({ cockpit, registry, backends: { pi, durable } as unknown as HubDeps["backends"], send: (m) => sent.push(m) });
  return {
    hub, sent, pi, durable, listeners, selected, actions,
    setSnapshot: (s: typeof snap) => { snap = s; },
    setActionResult: (f: () => Promise<unknown>) => { actionResult = f; },
    tree, inbox,
    subs: () => ({ subscriptions, disposedSubs }),
  };
};

const sessionMsg = (type: string, extra: Record<string, unknown> = {}) => ({ type, sessionId: "pi-1", ...extra });

describe("hub connection", () => {
  it("invalid client messages are rejected with a typed error", async () => {
    const t = setup();
    await t.hub.handle("not an object");
    await t.hub.handle({ type: "nope" });
    await t.hub.handle({ type: "session.submit", sessionId: "", clientSubmissionId: "c", text: "x", images: [], whenBusy: "followUp" });
    await t.hub.handle({ type: "cockpit.action", actionId: "a1", action: { kind: "no.such.action" } });

    const errors = t.sent.filter((m) => m.type === "session.error");
    expect(errors).toHaveLength(4);
    expect(t.sent).toHaveLength(4);
    for (const m of errors) expect(m.type === "session.error" && m.sessionId).toBeNull();
    const codes = errors.map((m) => (m.type === "session.error" ? m.error.code : ""));
    expect(codes.slice(0, 3)).toEqual(["invalid_message", "invalid_message", "invalid_message"]);
    expect(codes[3]).toBe("unknown_action");
    expect(t.pi.attaches).toHaveLength(0);
    expect(t.actions).toHaveLength(0);
  });

  it("cockpit subscribe pushes the current snapshot then invalidation updates", async () => {
    const t = setup();
    await t.hub.handle({ type: "cockpit.subscribe" });
    expect(t.sent).toEqual([{ type: "cockpit.snapshot", tree: t.tree, plan: null, inbox: t.inbox }]);
    expect(t.subs().subscriptions).toBe(1);

    t.listeners[0]?.({ kind: "pane", pane: "inbox", data: t.inbox });
    t.listeners[0]?.({ kind: "error", error: { code: "cpd_failed", message: "boom" } });
    expect(t.sent.slice(1)).toEqual([
      { type: "cockpit.patch", pane: "inbox", data: t.inbox },
      { type: "cockpit.error", error: { code: "cpd_failed", message: "boom" } },
    ]);

    await t.hub.handle({ type: "cockpit.subscribe" });
    expect(t.sent).toHaveLength(4);
    expect(t.sent[3]?.type).toBe("cockpit.snapshot");
    expect(t.subs().subscriptions).toBe(1);

    const notReady = setup();
    notReady.setSnapshot({ tree: null, plan: null, inbox: null });
    await notReady.hub.handle({ type: "cockpit.subscribe" });
    expect(notReady.sent).toHaveLength(1);
    expect(notReady.sent[0]).toMatchObject({ type: "cockpit.error", error: { code: "internal" } });
  });

  it("session attach selects the backend by session runner", async () => {
    const t = setup();
    await t.hub.handle({ type: "session.attach", sessionId: "pi-1" });
    expect(t.pi.attaches.map((a) => a.session.ref.id)).toEqual(["pi-1"]);
    expect(t.durable.attaches).toHaveLength(0);

    await t.hub.handle({ type: "session.attach", sessionId: "dur-1" });
    expect(t.durable.attaches.map((a) => a.session.ref.id)).toEqual(["dur-1"]);
    expect(t.pi.attaches[0]?.handle.disposed).toBe(1); // previous session detached first

    await t.hub.handle({ type: "session.attach", sessionId: "ghost" });
    const last = t.sent[t.sent.length - 1];
    expect(last).toMatchObject({ type: "session.error", sessionId: "ghost", error: { code: "session_not_found" } });
  });

  it("session messages route to the attached backend", async () => {
    const t = setup();
    await t.hub.handle({ type: "session.attach", sessionId: "pi-1" });
    const handle = t.pi.attaches[0]?.handle as FakeHandle;
    const submit = { clientSubmissionId: "c1", text: "hello", images: [], whenBusy: "followUp" };

    await t.hub.handle(sessionMsg("session.submit", submit));
    await t.hub.handle(sessionMsg("session.abort"));
    await t.hub.handle(sessionMsg("session.abortSubmission", { clientSubmissionId: "c1" }));
    await t.hub.handle(sessionMsg("session.compact", { instructions: "short" }));
    await t.hub.handle(sessionMsg("session.configure", { change: { kind: "tier", tier: "S2" } }));
    await t.hub.handle(sessionMsg("session.listModels"));
    await t.hub.handle(sessionMsg("session.loadEarlier", { before: "pi:100" }));
    await t.hub.handle(sessionMsg("dialog.respond", { dialogId: "d1", response: { method: "ask_user_question", outcome: "cancelled" } }));

    expect(handle.calls.map((c) => c[0])).toEqual(["submit", "abort", "abortSubmission", "compact", "configure", "listModels", "loadEarlier", "respond"]);
    expect(handle.calls[0]?.[1]).toEqual(submit);
    expect(handle.calls[2]?.[1]).toBe("c1");
    expect(handle.calls[3]?.[1]).toBe("short");
    expect(handle.calls[4]?.[1]).toEqual({ kind: "tier", tier: "S2" });
    expect(handle.calls[6]?.[1]).toBe("pi:100");
    expect(handle.calls[7]?.[1]).toBe("d1");

    const before = t.sent.length;
    await t.hub.handle({ type: "session.abort", sessionId: "other" });
    expect(handle.calls).toHaveLength(8);
    expect(t.sent.slice(before)).toMatchObject([{ type: "session.error", sessionId: "other", error: { code: "not_attached" } }]);

    await t.hub.handle({ type: "cockpit.selectPlan", planId: "p9" });
    expect(t.selected).toEqual(["p9"]);
    await t.hub.handle({ type: "cockpit.action", actionId: "a1", action: { kind: "inbox.dismiss", itemId: 4 } });
    expect(t.sent[t.sent.length - 1]).toEqual({ type: "action.result", actionId: "a1", ok: true, output: { stdout: "ok" } });
    t.setActionResult(() => Promise.reject(new WebError("cpd_failed", "nope", { cpdCode: "item_closed" })));
    await t.hub.handle({ type: "cockpit.action", actionId: "a2", action: { kind: "inbox.dismiss", itemId: 4 } });
    expect(t.sent[t.sent.length - 1]).toMatchObject({ type: "action.result", actionId: "a2", ok: false, error: { code: "cpd_failed" } });
  });

  it("detach and disconnect stop backend streams", async () => {
    const t = setup();
    await t.hub.handle({ type: "session.attach", sessionId: "pi-1" });
    const first = t.pi.attaches[0];
    const msg = { type: "session.footer", sessionId: "pi-1", footer: {} } as unknown as ServerMessage;

    first?.emit(msg as never);
    expect(t.sent).toEqual([msg]);

    await t.hub.handle({ type: "session.detach" });
    await t.hub.handle({ type: "session.detach" });
    expect(first?.handle.disposed).toBe(1);
    first?.emit(msg as never);
    expect(t.sent).toHaveLength(1); // nothing leaks after detach

    await t.hub.handle({ type: "session.attach", sessionId: "pi-2" });
    const second = t.pi.attaches[1];
    first?.emit(msg as never);
    expect(t.sent).toHaveLength(1); // an old attach never leaks into a newer one
    second?.emit(msg as never);
    expect(t.sent).toHaveLength(2);

    await t.hub.handle({ type: "cockpit.subscribe" });
    const afterSubscribe = t.sent.length;
    await t.hub.close();
    await t.hub.close();
    expect(second?.handle.disposed).toBe(1);
    expect(t.subs().disposedSubs).toBe(1);
    second?.emit(msg as never);
    await t.hub.handle({ type: "session.attach", sessionId: "pi-1" });
    await t.hub.handle({ type: "cockpit.subscribe" });
    expect(t.sent).toHaveLength(afterSubscribe);
    expect(t.pi.attaches).toHaveLength(2);
  });

  it("backend errors become session error messages", async () => {
    const t = setup();
    await t.hub.handle({ type: "session.attach", sessionId: "pi-1" });
    const handle = t.pi.attaches[0]?.handle as FakeHandle;

    handle.failWith = new WebError("cpd_failed", "send failed", { cpdCode: "x" });
    await t.hub.handle(sessionMsg("session.submit", { clientSubmissionId: "c7", text: "hi", images: [], whenBusy: "followUp" }));
    await t.hub.handle(sessionMsg("session.compact"));
    handle.failWith = new Error("kaput");
    await t.hub.handle(sessionMsg("session.abort"));

    expect(t.sent).toMatchObject([
      { type: "session.error", sessionId: "pi-1", clientSubmissionId: "c7", error: { code: "cpd_failed", message: "send failed" } },
      { type: "session.error", sessionId: "pi-1", error: { code: "cpd_failed" } },
      { type: "session.error", sessionId: "pi-1", error: { code: "internal", message: "kaput" } },
    ]);
    expect((t.sent[1] as { clientSubmissionId?: string }).clientSubmissionId).toBeUndefined();
  });

  describe("/q and /quit (decision pro-d406)", () => {
    const planTree = () => {
      const lead = { ref: { ...refOf("lead-1", "pi"), role: "lead" }, contextId: "c1", planId: "p1", jobKey: null, archived: false, dot: { state: "idle", attention: null } };
      const plan = { id: "p1", contextId: "c1", title: "P", phase: "plan", archived: false, counts: {}, leadSessionId: "lead-1", sessions: [lead] };
      return { contexts: [{ id: "c1", title: "C", archived: false, records: 0, openQuestions: 0, sessions: [], plans: [plan] }], cursor: 1 };
    };
    const attachTo = async (t: ReturnType<typeof setup>, sessionId: string) => {
      t.setSnapshot({ tree: planTree(), plan: null, inbox: t.inbox } as never);
      await t.hub.handle({ type: "session.attach", sessionId });
      return t.pi.attaches[t.pi.attaches.length - 1]?.handle as FakeHandle;
    };
    const submit = (t: ReturnType<typeof setup>, sessionId: string, id: string, text: string) =>
      t.hub.handle({ type: "session.submit", sessionId, clientSubmissionId: id, text, images: [], whenBusy: "followUp" });

    it("/q and /quit on a Plan Lead archive the Plan and never submit to the model", async () => {
      const t = setup();
      const handle = await attachTo(t, "lead-1");
      await submit(t, "lead-1", "c1", "/q");
      await submit(t, "lead-1", "c2", " /quit ");
      expect(t.actions).toEqual([{ kind: "plan.archive", planId: "p1" }, { kind: "plan.archive", planId: "p1" }]);
      expect(handle.calls.filter(([n]) => n === "submit")).toEqual([]);
      expect(t.sent.filter((m) => m.type === "session.error")).toEqual([]);
    });

    it("a refused archive of /q is reported and never submits to the model", async () => {
      const t = setup();
      const handle = await attachTo(t, "lead-1");
      t.setActionResult(() => Promise.reject(new WebError("cpd_failed", "plan has a running Job", { cpdCode: "archive_blocked" })));
      await submit(t, "lead-1", "c1", "/q");
      expect(t.actions).toEqual([{ kind: "plan.archive", planId: "p1" }]);
      expect(handle.calls.filter(([n]) => n === "submit")).toEqual([]);
      expect(t.sent).toMatchObject([{ type: "session.error", sessionId: "lead-1", clientSubmissionId: "c1", error: { message: expect.stringContaining("plan has a running Job") } }]);
    });

    it("/quit on a Manager is refused and never submits to the model", async () => {
      const t = setup();
      const handle = await attachTo(t, "mgr-1");
      await submit(t, "mgr-1", "c1", "/quit");
      expect(t.actions).toEqual([]);
      expect(handle.calls.filter(([n]) => n === "submit")).toEqual([]);
      expect(t.sent).toMatchObject([{ type: "session.error", sessionId: "mgr-1", clientSubmissionId: "c1", error: { message: expect.stringMatching(/manager/i) } }]);
    });

    it("text that only starts with /q is submitted unchanged", async () => {
      const t = setup();
      const handle = await attachTo(t, "lead-1");
      await submit(t, "lead-1", "c0", "/q"); // pins the intercept: /q itself is not submitted
      expect(handle.calls).toEqual([]);
      await submit(t, "lead-1", "c1", "/query");
      await submit(t, "lead-1", "c2", "/q now");
      expect(handle.calls.map(([n, i]) => [n, (i as { text: string }).text])).toEqual([["submit", "/query"], ["submit", "/q now"]]);
      expect(t.actions.length).toBe(1);
    });
  });
});

describe("hub cockpit readiness", () => {
  // air-n100/air-n99: a client that connects while the cockpit service is still reading the store (a
  // `loadAll` waiting on the setup lock) must get the snapshot once the read lands, never 'cockpit not ready'.
  it("a client that connects before the first load gets the snapshot", async () => {
    let completeFirstLoad: () => void = () => undefined;
    const firstLoad = new Promise<void>((resolve) => { completeFirstLoad = resolve; });
    const t = setup({ ready: firstLoad });
    // the first `readModel.loadAll` has not completed: the shared snapshot is still all-null
    t.setSnapshot({ tree: null, plan: null, inbox: null } as never);

    const connecting = t.hub.handle({ type: "cockpit.subscribe" });
    await flush();

    // a normal startup wait must never surface as 'cockpit not ready'
    expect(t.sent.filter((m) => m.type === "cockpit.error")).toEqual([]);

    // the first load lands and the client receives the snapshot
    t.setSnapshot({ tree: t.tree, plan: null, inbox: t.inbox } as never);
    completeFirstLoad();
    await connecting;

    expect(t.sent).toEqual([{ type: "cockpit.snapshot", tree: t.tree, plan: null, inbox: t.inbox }]);
    expect(t.subs().subscriptions).toBe(1); // and the client is subscribed to later updates
    t.listeners[0]?.({ kind: "pane", pane: "inbox", data: t.inbox });
    expect(t.sent[1]).toEqual({ type: "cockpit.patch", pane: "inbox", data: t.inbox });
  });

  it("the cockpit stays ready when the store read is lock-waited", async () => {
    let releaseLock: () => void = () => undefined;
    const lock = new Promise<void>((resolve) => { releaseLock = resolve; });
    const t = setup({ ready: lock });
    // the store read is waiting on the setup lock: tree and inbox are transiently null
    t.setSnapshot({ tree: null, plan: null, inbox: null } as never);

    const connecting = t.hub.handle({ type: "cockpit.subscribe" });
    await flush();

    // the lock wait is not a failure: nothing is sent while the read is in flight
    expect(t.sent.filter((m) => m.type === "cockpit.error")).toEqual([]);
    expect(t.sent).toEqual([]);

    // the read completes: the cockpit is ready and the client gets the snapshot
    t.setSnapshot({ tree: t.tree, plan: null, inbox: t.inbox } as never);
    releaseLock();
    await connecting;

    expect(t.sent.filter((m) => m.type === "cockpit.error")).toEqual([]);
    expect(t.sent).toEqual([{ type: "cockpit.snapshot", tree: t.tree, plan: null, inbox: t.inbox }]);
    expect(t.subs().subscriptions).toBe(1);
    t.listeners[0]?.({ kind: "pane", pane: "tree", data: t.tree });
    expect(t.sent[1]).toEqual({ type: "cockpit.patch", pane: "tree", data: t.tree });
  });
});
