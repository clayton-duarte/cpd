import { describe, expect, it, vi } from "vitest";
import type { Disposable } from "../../protocol/runtime.ts";
import type { Capabilities, Dialog, FooterStats, ModelChoice, Window } from "../../protocol/domain.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { createDurableBackend, type ResolvedSession, type SessionHandle } from "./backend.ts";
import type { HostClient, HostFrame } from "./client.ts";

type Obj = Record<string, unknown>;

class FakeHost {
  frames: HostFrame[] = [];
  handlers: ((f: HostFrame) => void)[] = [];
  pending: HostFrame[] = [];
  closeHandlers: ((e: Error | null) => void)[] = [];
  respond = vi.fn(async (_id: string, _answer: object) => {});
  configure = vi.fn(async (_change: object) => ({}) as Record<string, unknown>);
  submit = vi.fn(async (_input: object) => ({ submissionId: 7, status: "queued" }));
  history = vi.fn(async (_before?: string, _limit?: number): Promise<{ entries: readonly unknown[]; next?: string }> => ({ entries: [] }));
  abort = vi.fn(async () => {});
  abortSubmission = vi.fn(async (_id: number) => ({ outcome: "cancelled" }));
  compact = vi.fn(async (_i?: string) => ({ taskId: 1 }));
  detach = vi.fn(async () => {});
  closed = 0;
  models = [{ provider: "anthropic", model_id: "claude", name: "Claude", thinking_levels: ["off", "high"], context_window: 200000 }];
  listModels = vi.fn(async () => this.models);
  initial: HostFrame[];
  constructor(initial: HostFrame[]) {
    this.initial = initial;
  }
  deliver(frame: HostFrame): void {
    if (this.handlers.length === 0) this.pending.push(frame);
    for (const h of [...this.handlers]) h(frame);
  }
  client(): HostClient {
    const self = this;
    return {
      welcome: { type: "welcome", protocol: 2, host_version: "1", pi_durable: "1.5.0", session: "s1", role: "worker", epoch: "e1", client_id: "c1", cwd: "/work", limits: { client_line: 1e6 } },
      attach: async () => {
        for (const f of self.initial) self.deliver(f);
        return { type: "attached", req: "r1", mode: "view", epoch: "e1", from_seq: 0, head_seq: 1, client_id: "c1" };
      },
      submit: (i) => self.submit(i),
      compact: (i) => self.compact(i),
      configure: (c) => self.configure(c),
      listModels: () => self.listModels(),
      history: (b, l) => self.history(b, l),
      abort: () => self.abort(),
      abortSubmission: (id) => self.abortSubmission(id),
      respond: (id, a) => self.respond(id, a),
      detach: () => self.detach(),
      onFrame: (h): Disposable => {
        self.handlers.push(h);
        for (const f of self.pending.splice(0)) h(f);
        return { dispose: () => (self.handlers = self.handlers.filter((x) => x !== h)) };
      },
      onClose: (h): Disposable => {
        self.closeHandlers.push(h);
        return { dispose: () => {} };
      },
      close: () => {
        self.closed += 1;
      },
    };
  }
}

function userEntry(i: number): Obj {
  return { id: i, conversationId: "root", kind: "pi.user", model: [{ role: "user", content: `m${i}`, timestamp: i }] };
}
function viewFrame(entries: Obj[], opts: { before?: number; docs?: Obj; seq?: number } = {}): HostFrame {
  return {
    type: "view",
    epoch: "e1",
    seq: opts.seq ?? 1,
    view: {
      conversation: "root",
      entries,
      ...(opts.before === undefined ? {} : { before: opts.before }),
      docs: { "pi.agent": {}, "pi.live": {}, "pi.inbox": { items: [] }, "pi.provider": {}, "pi.usage": { models: {}, tools: {} }, ...opts.docs },
    },
  };
}
function opsFrame(ops: unknown[], seq = 2): HostFrame {
  return { type: "ops", epoch: "e1", seq, ops };
}
const RUN = { taskId: 1, inputs: [], turns: 0 };
const QUESTION_REQ: HostFrame = {
  type: "ui_request",
  id: "q1",
  purpose: "ask_user_question",
  created_at: 1,
  method: "questionnaire",
  questions: [{ header: "Pick", question: "Which?", options: [{ label: "A", description: "a" }, { label: "B", description: "b" }] }],
};

const SESSION: ResolvedSession = {
  ref: { id: "s1", key: "ctx:plan:job", runner: "durable", role: "worker", state: "working", title: "My chat", tier: "S2", cwd: "/work" },
  socketPath: "/run/host/s1.sock",
  tierOptions: ["S1", "S2"],
};

async function tick(): Promise<void> {
  for (let i = 0; i < 30; i++) await Promise.resolve();
}

async function setup(initial: HostFrame[], opts: { windowSize?: number } = {}) {
  const host = new FakeHost(initial);
  const cpd = { json: vi.fn(async (_args: readonly string[]) => ({}) as Readonly<Record<string, unknown>>) };
  const backend = createDurableBackend({
    connect: async (path) => {
      expect(path).toBe("/run/host/s1.sock");
      return host.client();
    },
    cpd,
    git: { branch: async () => "main" },
    scheduler: new FakeScheduler(),
    ...opts,
  });
  const messages: SessionServerMessage[] = [];
  const handle: SessionHandle = await backend.attach(SESSION, (m) => messages.push(m));
  await tick();
  return { host, cpd, handle, messages };
}

function ofType<T extends SessionServerMessage["type"]>(messages: SessionServerMessage[], type: T) {
  return messages.filter((m): m is Extract<SessionServerMessage, { type: T }> => m.type === type);
}
const ids = (w: Window) => w.items.map((i) => i.id);
const range = (a: number, b: number) => Array.from({ length: b - a }, (_, k) => `u:${a + k}`);

describe("durable backend", () => {
  it("attach returns a tail window with hasEarlier", async () => {
    const { messages } = await setup([viewFrame(Array.from({ length: 100 }, (_, i) => userEntry(i)))]);
    expect(messages.slice(0, 4).map((m) => m.type)).toEqual(["session.attached", "session.window", "session.footer", "session.status"]);
    const win = ofType(messages, "session.window")[0];
    expect(win?.mode).toBe("tail");
    expect(ids(win!.window)).toEqual(range(60, 100));
    expect(win!.window.hasEarlier).toBe(true);
    expect(win!.window.cursor).toBe("dur:u:60");
    const attached = ofType(messages, "session.attached")[0];
    expect(attached?.title).toBe("My chat");
    const small = await setup([viewFrame([userEntry(0), userEntry(1)])]);
    const w2 = ofType(small.messages, "session.window")[0]!.window;
    expect(w2.hasEarlier).toBe(false);
    expect(w2.cursor).toBeNull();
    const before = await setup([viewFrame([userEntry(10), userEntry(11)], { before: 9 })]);
    const w3 = ofType(before.messages, "session.window")[0]!.window;
    expect(w3.hasEarlier).toBe(true);
    expect(w3.cursor).toBe("dur:u:10");
  });

  it("loadEarlier pages older items from the cache then the history command", async () => {
    const entries = Array.from({ length: 40 }, (_, i) => userEntry(60 + i));
    const { handle, host, messages } = await setup([viewFrame(entries, { before: 59 })], { windowSize: 10 });
    expect(ids(ofType(messages, "session.window")[0]!.window)).toEqual(range(90, 100));
    await handle.loadEarlier("dur:u:90");
    expect(host.history).not.toHaveBeenCalled();
    const first = ofType(messages, "session.window").filter((m) => m.mode === "earlier")[0]!;
    expect(ids(first.window)).toEqual(range(80, 90));
    expect(first.window.cursor).toBe("dur:u:80");
    await handle.loadEarlier("dur:u:70");
    expect(host.history).not.toHaveBeenCalled();
    host.history.mockResolvedValueOnce({ entries: Array.from({ length: 30 }, (_, i) => userEntry(59 - i)), next: "n1" });
    await handle.loadEarlier("dur:u:60");
    expect(host.history).toHaveBeenCalledTimes(1);
    expect(host.history.mock.calls[0]?.[0]).toBeUndefined();
    const fromHistory = ofType(messages, "session.window").filter((m) => m.mode === "earlier").at(-1)!;
    expect(ids(fromHistory.window)).toEqual(range(50, 60));
    expect(fromHistory.window.hasEarlier).toBe(true);
    expect(fromHistory.window.cursor).toBe("dur:u:50");
    await handle.loadEarlier("dur:u:50");
    expect(host.history).toHaveBeenCalledTimes(1);
    await expect(handle.loadEarlier("garbage")).rejects.toMatchObject({ code: "invalid_message" });
    await expect(handle.loadEarlier("dur:u:999")).rejects.toMatchObject({ code: "invalid_message" });
  });

  it("a gap in the window is filled from history before the next append", async () => {
    const { host, messages } = await setup([viewFrame([userEntry(1), userEntry(2), userEntry(3)])]);
    const attachCount = messages.length;
    let release: (page: { entries: readonly unknown[]; next?: string }) => void = () => {};
    host.history.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    host.deliver(opsFrame([["r", (viewFrame([userEntry(20), userEntry(21), userEntry(22)], { before: 19 }) as { view: unknown }).view]]));
    host.deliver(opsFrame([["p", ["entries"], 3, 0, [userEntry(23)]]], 3));
    await tick();
    expect(host.history).toHaveBeenCalledTimes(1);
    expect(messages.slice(attachCount)).toEqual([]);
    release({ entries: Array.from({ length: 16 }, (_, i) => userEntry(19 - i)) });
    await tick();
    const after = messages.slice(attachCount).filter((m) => m.type === "session.window" || m.type === "session.append" || m.type === "session.update");
    expect(after.map((m) => m.type)).toEqual(["session.window", "session.append"]);
    const win = after[0] as Extract<SessionServerMessage, { type: "session.window" }>;
    expect(win.mode).toBe("tail");
    expect(ids(win.window)).toEqual(expect.arrayContaining(["u:3", "u:4", "u:19", "u:20", "u:22"]));
    expect(ids(win.window)).not.toContain("u:23");
    expect((after[1] as Extract<SessionServerMessage, { type: "session.append" }>).items.map((i) => i.id)).toEqual(["u:23"]);
  });

  it("ui_request becomes a dialog and ui_response answers it", async () => {
    const { handle, host, messages } = await setup([viewFrame([userEntry(0)])]);
    host.deliver(QUESTION_REQ);
    await tick();
    const dialog = ofType(messages, "session.dialog")[0]?.dialog as Dialog;
    expect(dialog).toMatchObject({ id: "q1", method: "ask_user_question", interactive: true });
    const result = { answers: [{ questionIndex: 0, question: "Which?", kind: "option" as const, answer: "A" }], cancelled: false };
    await handle.respond("q1", { method: "ask_user_question", outcome: "answered", result });
    expect(host.respond).toHaveBeenCalledWith("q1", { answer: result });
    await handle.respond("q1", { method: "ask_user_question", outcome: "cancelled" });
    expect(host.respond).toHaveBeenLastCalledWith("q1", { cancelled: true });
    await expect(handle.respond("nope", { method: "ask_user_question", outcome: "cancelled" })).rejects.toMatchObject({ code: "invalid_message" });
  });

  it("ui_resolved clears the dialog", async () => {
    const { host, messages } = await setup([viewFrame([userEntry(0)]), { ...QUESTION_REQ, replayed: true } as HostFrame]);
    expect(ofType(messages, "session.dialog").map((m) => m.dialog.id)).toEqual(["q1"]);
    host.deliver({ type: "ui_resolved", id: "q1", outcome: "answered" });
    await tick();
    expect(ofType(messages, "session.dialogResolved")).toEqual([{ type: "session.dialogResolved", sessionId: "s1", dialogId: "q1", outcome: "answered" }]);
  });

  it("configure sends model and thinking level", async () => {
    const { handle, host } = await setup([viewFrame([userEntry(0)])]);
    await handle.configure({ kind: "model", model: "anthropic/claude" });
    expect(host.configure).toHaveBeenLastCalledWith({ model: "anthropic/claude" });
    await handle.configure({ kind: "thinking", level: "high" });
    expect(host.configure).toHaveBeenLastCalledWith({ thinkingLevel: "high" });
    await handle.configure({ kind: "thinking", level: null });
    expect(host.configure).toHaveBeenLastCalledWith({ thinkingLevel: null });
  });

  it("list_models returns model choices", async () => {
    const { handle, messages } = await setup([viewFrame([userEntry(0)])]);
    await handle.listModels();
    const models = ofType(messages, "session.models").at(-1)?.models as ModelChoice[];
    expect(models).toEqual([{ model: "anthropic/claude", name: "Claude", thinkingLevels: ["off", "high"], contextWindow: 200000 }]);
  });

  it("capabilities declare images, configure, abort and dialogs as supported", async () => {
    const { handle, messages } = await setup([viewFrame([userEntry(0)])]);
    const caps = ofType(messages, "session.attached")[0]?.capabilities as Capabilities;
    for (const c of [caps, handle.capabilities]) {
      expect(c.images).toMatchObject({ supported: true, maxCount: 4, maxTotalBase64: 786432 });
      expect(c.configure.model.supported).toBe(true);
      expect(c.configure.thinking.supported).toBe(true);
      expect(c.configure.tier).toMatchObject({ supported: true, options: ["S1", "S2"] });
      expect(c.abort.supported).toBe(true);
      expect(c.abortSubmission.supported).toBe(true);
      expect(c.steer.supported).toBe(true);
      expect(c.followUp.supported).toBe(true);
      expect(c.compact.supported).toBe(true);
      expect(c.dialogs).toEqual({ mode: "interactive" });
    }
    const footer = ofType(messages, "session.footer")[0]?.footer as FooterStats;
    expect(footer.branch).toEqual({ state: "known", value: "main" });
    expect(footer.tier).toEqual({ state: "known", value: "S2" });
  });

  it("queued submissions come from the inbox doc", async () => {
    const { handle, host, messages } = await setup([viewFrame([userEntry(0)])]);
    host.deliver(opsFrame([["s", ["docs", "pi.live", "run"], RUN]]));
    await tick();
    expect(ofType(messages, "session.status").at(-1)?.busy).toBe(true);
    await handle.submit({ clientSubmissionId: "cs1", text: "later", images: [], whenBusy: "followUp" });
    expect(host.submit).toHaveBeenCalledWith(expect.objectContaining({ text: "later", whenBusy: "followUp", requestId: "cs1" }));
    expect(ofType(messages, "session.submitAck")).toEqual([{ type: "session.submitAck", sessionId: "s1", clientSubmissionId: "cs1", queued: true, submissionId: 7 }]);
    const queued = { clientSubmissionId: "cs1", submissionId: 7, text: "later", whenBusy: "followUp" };
    expect(ofType(messages, "session.status").at(-1)?.queued).toEqual([queued]);
    host.deliver(opsFrame([["s", ["docs", "pi.inbox", "items"], [{ id: 7, mode: "followUp", content: "later" }, { id: 8, mode: "write", entry: 3 }]]], 3));
    await tick();
    expect(ofType(messages, "session.status").at(-1)?.queued).toEqual([queued]);
    host.deliver(opsFrame([["s", ["docs", "pi.inbox", "items"], []]], 4));
    await tick();
    expect(ofType(messages, "session.status").at(-1)?.queued).toEqual([]);
  });
});
