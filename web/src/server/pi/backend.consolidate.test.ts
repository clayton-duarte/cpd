import { describe, expect, it, vi } from "vitest";
import type { Disposable } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { createDurableBackend } from "../durable/backend.ts";
import type { HostClient, HostFrame } from "../durable/client.ts";
import type { SessionHandle } from "../hub/connection.ts";
import type { ResolvedSession } from "../hub/registry.ts";

// CONTRACT 11.4 "durable plan review": only the host-held request can open an actionable review.

type Obj = Record<string, unknown>;

class FakeHost {
  handlers: ((f: HostFrame) => void)[] = [];
  pending: HostFrame[] = [];
  respond = vi.fn(async (_id: string, _answer: object) => {});
  submit = vi.fn(async (_input: object) => ({ submissionId: 7, status: "queued" }));
  detach = vi.fn(async () => {});
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
      compact: async () => ({ taskId: 1 }),
      configure: async () => ({}),
      listModels: async () => [],
      history: async () => ({ entries: [] }),
      abort: async () => {},
      abortSubmission: async () => ({ outcome: "cancelled" }),
      respond: (id, a) => self.respond(id, a),
      detach: () => self.detach(),
      onFrame: (h): Disposable => {
        self.handlers.push(h);
        for (const f of self.pending.splice(0)) h(f);
        return { dispose: () => (self.handlers = self.handlers.filter((x) => x !== h)) };
      },
      onClose: (): Disposable => ({ dispose: () => {} }),
      close: () => {},
    };
  }
}

const userEntry = (i: number): Obj => ({ id: i, conversationId: "root", kind: "pi.user", model: [{ role: "user", content: `m${i}`, timestamp: i }] });
const reviewEntry = (id: number, n: string): Obj => ({ id, conversationId: "root", kind: "cpd-plan-review", data: { review: n, status: "pending" } });
const viewFrame = (entries: Obj[]): HostFrame => ({
  type: "view",
  epoch: "e1",
  seq: 1,
  view: { conversation: "root", entries, docs: { "pi.agent": {}, "pi.live": {}, "pi.inbox": { items: [] }, "pi.provider": {}, "pi.usage": { models: {}, tools: {} } } },
});
const appendEntry = (at: number, entry: Obj, seq: number): HostFrame => ({ type: "ops", epoch: "e1", seq, ops: [["p", ["entries"], at, 0, [entry]]] });
const reviewRequest = (id: string): HostFrame => ({
  type: "ui_request", id, purpose: "plan_review", method: "select",
  title: "Approve the plan review", options: ["Approve", "Change"], created_at: 1,
});

const SESSION: ResolvedSession = {
  ref: { id: "s1", key: "ctx:plan:job", runner: "durable", role: "worker", state: "working", title: "My chat", tier: "S2", cwd: "/work" },
  socketPath: "/run/host/s1.sock",
  tierOptions: ["S1", "S2"],
};

async function tick(): Promise<void> {
  for (let i = 0; i < 30; i++) await Promise.resolve();
}

async function setup(initial: HostFrame[]) {
  const host = new FakeHost(initial);
  const cpd = { json: vi.fn(async (_args: readonly string[]) => ({}) as Readonly<Record<string, unknown>>) };
  const backend = createDurableBackend({ connect: async () => host.client(), cpd, git: { branch: async () => "main" }, scheduler: new FakeScheduler() });
  const messages: SessionServerMessage[] = [];
  const handle: SessionHandle = await backend.attach(SESSION, (m) => messages.push(m));
  await tick();
  return { host, cpd, handle, messages };
}
const dialogs = (messages: SessionServerMessage[]) => messages.filter((m): m is Extract<SessionServerMessage, { type: "session.dialog" }> => m.type === "session.dialog");
const REVIEW = { method: "cpd_plan_review" } as const;

describe("durable backend plan reviews", () => {
  it("does not open or decide a review from a transcript entry alone", async () => {
    const { cpd, host, handle, messages } = await setup([viewFrame([userEntry(1), reviewEntry(2, "7")])]);
    expect(dialogs(messages)).toEqual([]);
    await expect(handle.respond("review:7", { ...REVIEW, outcome: "approve" })).rejects.toMatchObject({ code: "invalid_message" });
    expect(cpd.json).not.toHaveBeenCalledWith(expect.arrayContaining(["plan", "decide"]));
    expect(host.respond).not.toHaveBeenCalled();
  });

  it("sends approval only to the held request and waits for host resolution", async () => {
    const { cpd, host, handle, messages } = await setup([viewFrame([reviewEntry(1, "7")]), reviewRequest("held-7")]);
    expect(dialogs(messages).map((m) => m.dialog.id)).toEqual(["held-7"]);
    await handle.respond("held-7", { ...REVIEW, outcome: "approve" });
    expect(host.respond.mock.calls).toEqual([["held-7", { value: "Approve" }]]);
    expect(cpd.json).not.toHaveBeenCalledWith(expect.arrayContaining(["plan", "decide"]));
    expect(messages.some((m) => m.type === "session.dialogResolved")).toBe(false);
    host.deliver({ type: "ui_resolved", id: "held-7", outcome: "answered", by: "client" });
    await tick();
    expect(messages).toContainEqual({ type: "session.dialogResolved", sessionId: "s1", dialogId: "held-7", outcome: "answered" });
    await expect(handle.respond("held-7", { ...REVIEW, outcome: "approve" })).rejects.toMatchObject({ code: "invalid_message" });
    expect(host.respond).toHaveBeenCalledTimes(1);
  });

  it("uses the held feedback editor without sending a follow-up chat message", async () => {
    const { cpd, host, handle } = await setup([viewFrame([reviewEntry(1, "7")]), reviewRequest("held-7")]);
    await handle.respond("held-7", { ...REVIEW, outcome: "changes", feedback: "rename the step" });
    host.deliver({ type: "ui_request", id: "held-editor", method: "editor", purpose: "plan_review", created_at: 2, title: "What should change?" });
    await tick();
    expect(host.respond.mock.calls).toEqual([["held-7", { value: "Change" }], ["held-editor", { value: "rename the step" }]]);
    expect(cpd.json).not.toHaveBeenCalledWith(expect.arrayContaining(["plan", "decide"]));
    expect(host.submit).not.toHaveBeenCalled();
  });

  it("ignores transcript and held review frames after dispose", async () => {
    const { host, handle, messages } = await setup([viewFrame([userEntry(1)])]);
    host.deliver(appendEntry(1, reviewEntry(2, "7"), 2));
    host.deliver(reviewRequest("held-7"));
    await tick();
    expect(dialogs(messages).map((m) => m.dialog.id)).toEqual(["held-7"]);
    await handle.dispose();
    const seen = messages.length;
    host.deliver(appendEntry(2, reviewEntry(3, "8"), 3));
    host.deliver(reviewRequest("held-8"));
    await tick();
    expect(messages).toHaveLength(seen);
  });
});
