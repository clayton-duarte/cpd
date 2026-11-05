import { describe, expect, it, vi } from "vitest";
import type { Disposable } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { createDurableBackend } from "../durable/backend.ts";
import type { HostClient, HostFrame } from "../durable/client.ts";
import type { SessionHandle } from "../hub/connection.ts";
import type { ResolvedSession } from "../hub/registry.ts";

// CONTRACT 11.4 "durable plan review": the durable backend syncs a transcript `cpd-plan-review` entry
// as the dialog `review:<n>` and answers it through `cpd plan decide` (and a follow-up submit).

type Obj = Record<string, unknown>;

class FakeHost {
  handlers: ((f: HostFrame) => void)[] = [];
  pending: HostFrame[] = [];
  respond = vi.fn(async (_id: string, _answer: object) => {});
  submit = vi.fn(async (_input: object) => ({ submissionId: 7, status: "queued" }));
  detach = vi.fn(async () => {});
  constructor(private readonly initial: HostFrame[]) {}
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

describe("durable backend transcript plan reviews", () => {
  it("pi backend syncs a transcript plan review as a review dialog", async () => {
    const { messages } = await setup([viewFrame([userEntry(1), reviewEntry(2, "7")])]);
    expect(dialogs(messages).map((m) => m.dialog)).toEqual([
      { id: "review:7", method: "cpd_plan_review", title: "Plan review 7", summary: "", interactive: true },
    ]);
  });

  it("pi backend approves a review dialog with cpd plan decide approve", async () => {
    const { cpd, host, handle, messages } = await setup([viewFrame([reviewEntry(1, "7")])]);
    await handle.respond("review:7", { ...REVIEW, outcome: "approve" });
    expect(cpd.json).toHaveBeenCalledWith(["plan", "decide", "7", "--approve", "--plan", "plan"]);
    expect(host.respond).not.toHaveBeenCalled();
    expect(messages).toContainEqual({ type: "session.dialogResolved", sessionId: "s1", dialogId: "review:7", outcome: "answered" });
  });

  it("pi backend changes outcome rejects the review and follows up with the feedback", async () => {
    const { cpd, host, handle } = await setup([viewFrame([reviewEntry(1, "7")])]);
    await handle.respond("review:7", { ...REVIEW, outcome: "changes", feedback: "rename the step" });
    expect(cpd.json).toHaveBeenCalledWith(["plan", "decide", "7", "--reject", "--plan", "plan"]);
    expect(host.submit).toHaveBeenCalledWith({ text: "rename the step", whenBusy: "followUp", requestId: "feedback:review:7" });
    expect(host.respond).not.toHaveBeenCalled();
  });

  it("pi backend ignores frames after dispose", async () => {
    const { host, handle, messages } = await setup([viewFrame([userEntry(1)])]);
    host.deliver(appendEntry(1, reviewEntry(2, "7"), 2));
    await tick();
    expect(dialogs(messages).map((m) => m.dialog.id)).toEqual(["review:7"]);
    await handle.dispose();
    const seen = messages.length;
    host.deliver(appendEntry(2, reviewEntry(3, "8"), 3));
    await tick();
    expect(messages).toHaveLength(seen);
  });
});
