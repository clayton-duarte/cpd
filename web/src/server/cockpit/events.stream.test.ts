import { describe, expect, it } from "vitest";
import { FakeScheduler } from "../../testing/scheduler.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import type { Cpd, FollowBoundary, FollowProcess } from "./cli.ts";
import { buildPlanView, createReadModel, parsePlanShow } from "./readmodel.ts";
import { createCockpitService, type CockpitUpdate, type CpdEvent } from "./events.ts";
import type { CockpitStream, StreamSnapshot } from "./stream.ts";

type Obj = Record<string, unknown>;

class FakeFollow implements FollowProcess {
  private lineHandlers: ((l: string) => void)[] = [];
  onLine(h: (l: string) => void): Disposable { this.lineHandlers.push(h); return { dispose: () => { this.lineHandlers = this.lineHandlers.filter((x) => x !== h); } }; }
  onExit(): Disposable { return { dispose: () => undefined }; }
  kill(): void { /* recorded by nothing: not under test */ }
  emit(line: string): void { for (const h of [...this.lineHandlers]) h(line); }
}

class FakeStream implements CockpitStream {
  isReady = true;
  started = 0;
  planShows: string[] = [];
  planShowResult: unknown = "down";
  private snapshotCbs: ((s: StreamSnapshot) => void)[] = [];
  private eventCbs: ((e: CpdEvent) => void)[] = [];
  start(): void { this.started++; }
  stop(): void { /* nothing */ }
  ready(): boolean { return this.isReady; }
  onSnapshot(cb: (s: StreamSnapshot) => void): void { this.snapshotCbs.push(cb); }
  onEvent(cb: (e: CpdEvent) => void): void { this.eventCbs.push(cb); }
  onError(): void { /* not under test */ }
  planShow(id: string): Promise<unknown> { this.planShows.push(id); return Promise.resolve(this.planShowResult); }
  snapshot(panes: StreamSnapshot["panes"], seq = 1): void { for (const cb of this.snapshotCbs) cb({ seq, panes }); }
  event(e: CpdEvent): void { for (const cb of this.eventCbs) cb(e); }
}

const context = (id: string, title = `Title ${id}`): Obj => ({ id, title, archived_at: null, records: 2, open_questions: 1 });
const session = (id: string, state: string): Obj => ({
  id, key: null, name: null, runner: "pi", role: "manager", state, context: "c1", plan: null, plan_lead: false,
  archived_at: null, file: null, tier: null, title: null, cwd: null,
});
const panesOf = (title: string, state: string): StreamSnapshot["panes"] => ({
  context: { ok: true, contexts: [context("c1", title)], events_cursor: 10 },
  plan: { ok: true, plans: [], events_cursor: 8 },
  session: { ok: true, sessions: [session("s1", state)], events_cursor: 12 },
  inbox: { ok: true, items: [], engine: { running: true }, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, events_cursor: 9 },
});
const planShowValue: Obj = {
  ok: true,
  plan: { id: "p1", context: "c1", title: "Plan p1", phase: "dispatch", archived_at: null, lead_session: null },
  counts: { draft: 0, queued: 1, running: 0, merged: 0, failed: 0, cancelled: 0 },
  jobs: [], workflows: [], review: null, escalations: [], events_cursor: 20,
};

function fakeCpd(panes: StreamSnapshot["panes"]): { cpd: Cpd; calls: string[] } {
  const calls: string[] = [];
  const cpd: Cpd = {
    async json(args) {
      const key = args.join(" ");
      calls.push(key);
      switch (key) {
        case "context list": return panes.context as Obj;
        case "plan list": return panes.plan as Obj;
        case "session list": return panes.session as Obj;
        case "inbox list": return panes.inbox as Obj;
        default:
          if (key.startsWith("plan show")) return planShowValue;
          throw new Error(`unexpected ${key}`);
      }
    },
  };
  return { cpd, calls };
}

const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
const ev = (id: number, kind: string, extra: Partial<CpdEvent> = {}): CpdEvent => ({
  id, kind, plan: null, context: null, session: null, job: null, attempt: null, at: "2026-01-01T00:00:00Z", text: "", data: {}, ...extra,
});
const paneNames = (us: CockpitUpdate[]) => us.map((u) => (u.kind === "pane" ? u.pane : u.kind));

async function setup(stream: FakeStream, initial = panesOf("initial", "idle")) {
  const scheduler = new FakeScheduler();
  const follows: FakeFollow[] = [];
  const follow: FollowBoundary = { start: () => { const p = new FakeFollow(); follows.push(p); return p; } };
  const { cpd, calls } = fakeCpd(initial);
  const service = createCockpitService({ readModel: createReadModel(cpd), cpd, follow, scheduler, stream });
  await service.start();
  const updates: CockpitUpdate[] = [];
  service.subscribe((u) => updates.push(u));
  return { service, scheduler, follows, calls, updates };
}

describe("cockpit service stream", () => {
  it("a snapshot frame emits the panes mapped by the read model", async () => {
    const stream = new FakeStream();
    const { service, scheduler, updates } = await setup(stream);
    expect(stream.started).toBe(1);
    const raw = panesOf("streamed", "working");
    const expected = await createReadModel(fakeCpd(raw).cpd).loadPanes(new Set(["tree", "inbox"]), null);
    stream.snapshot(raw);
    scheduler.advance(16);
    await flush();
    expect(updates).toEqual([
      { kind: "pane", pane: "tree", data: expected.tree },
      { kind: "pane", pane: "inbox", data: expected.inbox },
    ]);
    service.stop();
  });

  it("snapshot panes are emitted within the sixteen millisecond window and merged", async () => {
    const stream = new FakeStream();
    const { service, scheduler, updates } = await setup(stream);
    stream.snapshot(panesOf("first", "idle"), 1);
    scheduler.advance(10);
    stream.snapshot(panesOf("second", "idle"), 2);
    scheduler.advance(5);
    await flush();
    expect(updates).toEqual([]);
    scheduler.advance(1);
    await flush();
    expect(paneNames(updates)).toEqual(["tree", "inbox"]);
    const treeUpdate = updates[0]!;
    expect(treeUpdate.kind === "pane" && treeUpdate.pane === "tree" && treeUpdate.data.contexts[0]!.title).toBe("second");
    service.stop();
  });

  it("a session state event updates the session node state and dot in the same tick", async () => {
    const stream = new FakeStream();
    const { service, scheduler, updates } = await setup(stream);
    stream.snapshot(panesOf("c", "idle"));
    scheduler.advance(16);
    await flush();
    updates.length = 0;
    stream.event(ev(50, "session-state", { context: "c1", session: "s1", data: { from: "idle", to: "working" } }));
    expect(paneNames(updates)).toEqual(["tree"]);
    const u = updates[0]!;
    const node = u.kind === "pane" && u.pane === "tree" ? u.data.contexts[0]!.sessions[0]! : null;
    expect(node?.ref.state).toBe("working");
    expect(node?.dot).toEqual({ state: "working", attention: null });
    service.stop();
  });

  it("the next snapshot reconciles an in memory session state", async () => {
    const stream = new FakeStream();
    const { service, scheduler, updates } = await setup(stream);
    stream.snapshot(panesOf("c", "idle"));
    scheduler.advance(16);
    await flush();
    stream.event(ev(50, "session-state", { context: "c1", session: "s1", data: { from: "idle", to: "working" } }));
    updates.length = 0;
    stream.snapshot(panesOf("c", "blocked"), 2);
    scheduler.advance(16);
    await flush();
    const u = updates.find((x) => x.kind === "pane" && x.pane === "tree")!;
    const node = u.kind === "pane" && u.pane === "tree" ? u.data.contexts[0]!.sessions[0]! : null;
    expect(node?.ref.state).toBe("blocked");
    service.stop();
  });

  it("while the stream is not ready the spawn refresh serves the panes", async () => {
    const stream = new FakeStream();
    stream.isReady = false;
    const { service, scheduler, follows, calls, updates } = await setup(stream);
    expect(follows).toHaveLength(1);
    calls.length = 0;
    follows[0]!.emit(JSON.stringify(ev(60, "inbox-opened")));
    scheduler.advance(16);
    await flush();
    expect(calls).toContain("inbox list");
    expect(paneNames(updates)).toContain("inbox");
    service.stop();
  });

  it("the spawn fallback coalesces at sixteen milliseconds", async () => {
    const stream = new FakeStream();
    stream.isReady = false;
    const { service, scheduler, follows, calls } = await setup(stream);
    calls.length = 0;
    for (let i = 0; i < 5; i++) follows[0]!.emit(JSON.stringify(ev(60 + i, "inbox-opened")));
    scheduler.advance(15);
    await flush();
    expect(calls).toEqual([]);
    scheduler.advance(1);
    await flush();
    expect(calls.filter((c) => c === "inbox list")).toHaveLength(1);
    service.stop();
  });

  it("select plan asks the stream for plan show", async () => {
    const stream = new FakeStream();
    stream.planShowResult = planShowValue;
    const { service, calls, updates } = await setup(stream);
    calls.length = 0;
    await service.selectPlan("p1");
    await flush();
    expect(stream.planShows).toEqual(["p1"]);
    expect(calls.filter((c) => c.startsWith("plan show"))).toEqual([]);
    expect(updates).toContainEqual({ kind: "pane", pane: "plan", data: buildPlanView(parsePlanShow(planShowValue)) });
    service.stop();
  });
});
