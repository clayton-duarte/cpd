import { describe, expect, it } from "vitest";
import { FakeScheduler } from "../../testing/scheduler.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import type { ContextTreeModel, InboxModel, PlanViewModel } from "../../protocol/cockpit.ts";
import type { Cpd, FollowBoundary, FollowProcess } from "./cli.ts";
import type { PaneSet, PaneUpdate, ReadModel } from "./readmodel.ts";
import type { CockpitStream, StreamSnapshot } from "./stream.ts";
import { COALESCE_MS, SAFETY_REFRESH_MS, FOLLOW_RESTART_BASE_MS, createCockpitService, type CockpitUpdate, type Extras } from "./events.ts";
import type { StatusPane } from "./status.ts";

const tree = (cursor: number): ContextTreeModel => ({ contexts: [], cursor });
const inbox: InboxModel = { openCount: 0, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: [], cursor: 0 };

class FakeFollow implements FollowProcess {
  killed = false;
  private lineHandlers: ((l: string) => void)[] = [];
  private exitHandlers: ((i: { code: number | null; stderr: string }) => void)[] = [];
  readonly args: readonly string[];
  constructor(args: readonly string[]) { this.args = args; }
  onLine(h: (l: string) => void): Disposable { this.lineHandlers.push(h); return { dispose: () => { this.lineHandlers = this.lineHandlers.filter((x) => x !== h); } }; }
  onExit(h: (i: { code: number | null; stderr: string }) => void): Disposable { this.exitHandlers.push(h); return { dispose: () => { this.exitHandlers = this.exitHandlers.filter((x) => x !== h); } }; }
  kill(): void { this.killed = true; }
  emit(line: string): void { for (const h of [...this.lineHandlers]) h(line); }
  exit(code: number | null): void { for (const h of [...this.exitHandlers]) h({ code, stderr: "" }); }
}

function setup(gate?: () => Promise<void>, cpdOverride?: Cpd) {
  const scheduler = new FakeScheduler();
  const procs: FakeFollow[] = [];
  const follow: FollowBoundary = { start: (args) => { const p = new FakeFollow(args); procs.push(p); return p; } };
  const loads: { panes: string[]; planId: string | null }[] = [];
  let all = 0;
  const readModel: ReadModel = {
    async loadAll() { all++; return { tree: tree(40), plan: null, inbox }; },
    async loadPanes(panes: PaneSet, planId) {
      loads.push({ panes: [...panes].sort(), planId });
      if (gate !== undefined) await gate();
      const out: { tree?: ContextTreeModel; plan?: PlanViewModel | null; inbox?: InboxModel } = {};
      if (panes.has("tree")) out.tree = tree(41);
      if (panes.has("inbox")) out.inbox = inbox;
      if (panes.has("plan")) out.plan = null;
      return out;
    },
  };
  const cpd: Cpd = cpdOverride ?? { json: async () => ({ ok: true }) };
  const service = createCockpitService({ readModel, cpd, follow, scheduler });
  const updates: CockpitUpdate[] = [];
  service.subscribe((u) => updates.push(u));
  return { service, scheduler, procs, loads, updates, allCount: () => all };
}

const ev = (id: number, kind: string) => JSON.stringify({ id, kind, plan: null, context: null, session: null, job: null, attempt: null, at: "2026-01-01T00:00:00Z", text: "" });
const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };

/** A cockpit stream that is already ready: the primary path of 13, no follow process. */
class ReadyStream implements CockpitStream {
  private snapshotCbs: ((s: StreamSnapshot) => void)[] = [];
  start(): void { /* not under test */ }
  stop(): void { /* not under test */ }
  ready(): boolean { return true; }
  onSnapshot(cb: (s: StreamSnapshot) => void): void { this.snapshotCbs.push(cb); }
  onEvent(): void { /* not under test */ }
  onError(): void { /* not under test */ }
  planShow(): Promise<unknown> { return Promise.resolve("down"); }
  snapshot(panes: StreamSnapshot["panes"], seq: number): void { for (const cb of [...this.snapshotCbs]) cb({ seq, panes }); }
}

type Raw = Record<string, unknown>;
const ctxRow = (id: string): Raw => ({ id, title: `Title ${id}`, archived_at: null, records: 0, open_questions: 0 });
const planRow = (id: string, context: string): Raw => ({
  id, context, title: `Plan ${id}`, phase: "dispatch", archived_at: null, created_at: "2026-01-01T00:00:00Z",
  lead_session: null, counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 },
});
const rawPanes = (contexts: Raw[], plans: Raw[], c: { context: number; plan: number; session: number; inbox: number }): StreamSnapshot["panes"] => ({
  context: { ok: true, contexts, events_cursor: c.context },
  plan: { ok: true, plans, events_cursor: c.plan },
  session: { ok: true, sessions: [], events_cursor: c.session },
  inbox: { ok: true, items: [], engine: { running: true }, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, events_cursor: c.inbox },
});

describe("cockpit service events", () => {
  it("events follow starts once and resumes from the snapshot cursor", async () => {
    const { service, procs } = setup();
    await service.start();
    await service.start();
    expect(procs).toHaveLength(1);
    expect(procs[0]!.args).toEqual(["events", "follow", "--since", "40"]);
    service.stop();
  });

  it("an event kind invalidates only its panes", async () => {
    const { service, procs, scheduler, loads, updates } = setup();
    await service.start();
    procs[0]!.emit(ev(41, "context-created"));
    scheduler.advance(COALESCE_MS);
    await flush();
    expect(loads).toEqual([{ panes: ["tree"], planId: null }]);
    expect(updates.map((u) => (u.kind === "pane" ? u.pane : u.kind))).toEqual(["tree"]);
    procs[0]!.emit(ev(42, "session-titled"));
    scheduler.advance(COALESCE_MS);
    await flush();
    expect(loads).toHaveLength(1);
    service.stop();
  });

  it("a burst of events triggers one coalesced refresh", async () => {
    const { service, procs, scheduler, loads } = setup();
    await service.start();
    for (let i = 0; i < 5; i++) procs[0]!.emit(ev(41 + i, "inbox-opened"));
    expect(loads).toEqual([]);
    scheduler.advance(COALESCE_MS);
    await flush();
    expect(loads).toHaveLength(1);
    expect(loads[0]!.panes).toEqual(["inbox", "tree"]);
    service.stop();
  });

  it("a dropped follow process restarts from the last cursor", async () => {
    const { service, procs, scheduler } = setup();
    await service.start();
    procs[0]!.emit(ev(57, "session-titled"));
    procs[0]!.exit(1);
    expect(procs).toHaveLength(1);
    scheduler.advance(FOLLOW_RESTART_BASE_MS);
    expect(procs).toHaveLength(2);
    expect(procs[1]!.args).toEqual(["events", "follow", "--since", "57"]);
    service.stop();
  });

  it("stopping the service stops the follow process", async () => {
    const { service, procs, scheduler, loads, updates } = setup();
    await service.start();
    procs[0]!.emit(ev(41, "context-created"));
    service.stop();
    service.stop();
    expect(procs[0]!.killed).toBe(true);
    scheduler.advance(120_000);
    await flush();
    expect(loads).toEqual([]);
    expect(updates).toEqual([]);
    expect(scheduler.pending()).toBe(0);
  });

  it("a snapshot's status pane reaches the emitted status and is emitted only on change", async () => {
    const scheduler = new FakeScheduler();
    const readModel: ReadModel = {
      async loadAll() { return { tree: tree(1), plan: null, inbox }; },
      async loadPanes() { return {}; },
    };
    const cpd: Cpd = { json: async () => ({ ok: true }) };
    const follow: FollowBoundary = { start: () => { throw new Error("no follow process in this test"); } };
    const pane: StatusPane = { workers: { queued: 3, waiting: 2, running: 9, limit: 6 }, incident: { on: true, started_at: "2026-10-14T21:10:00.000Z", lead: "lead-1" } };
    const seen: (StatusPane | null)[] = [];
    const extras: Extras = {
      async status(p) {
        seen.push(p);
        return {
          runner: null,
          preview: null,
          incident: p === null || !p.incident.on || p.incident.started_at === null ? null : { startedAt: p.incident.started_at, lead: p.incident.lead },
          workers: p === null ? null : { queued: p.workers.queued, waiting: p.workers.waiting, tenants: p.workers.running, limit: p.workers.limit },
          version: null,
          hostAlert: false,
          mode: "production",
          drift: null,
        };
      },
      async costs() { return {}; },
      async logs() { return []; },
      async settings() { throw new Error("settings are not read in this test"); },
      async saveSettings() {},
    };
    const stream = new ReadyStream();
    const service = createCockpitService({ readModel, cpd, follow, scheduler, extras, stream });
    const updates: CockpitUpdate[] = [];
    service.subscribe((u) => updates.push(u));
    await service.start();
    await flush();
    const raw = rawPanes([], [], { context: 1, plan: 1, session: 1, inbox: 1 });
    stream.snapshot({ ...raw, status: { ok: true, ...pane } }, 1);
    await flush();
    stream.snapshot({ ...raw, status: { ok: true, ...pane } }, 2);
    await flush();
    expect(seen).toHaveLength(2);
    expect(seen[0]).toBeNull();
    expect(seen[1]).toMatchObject(pane);
    const statusUpdates = updates.filter((u) => u.kind === "pane" && u.pane === "status");
    expect(statusUpdates).toHaveLength(2);
    expect(statusUpdates[1]).toMatchObject({ data: { workers: { queued: 3, waiting: 2, tenants: 9, limit: 6 }, incident: { startedAt: "2026-10-14T21:10:00.000Z", lead: "lead-1" } } });
    service.stop();
  });
});

const TREE_EVENT_KINDS = "context-created context-grounded context-record context-renamed context-archived context-curated context-owner context-deleted plan-created plan-phase plan-done plan-renamed plan-archived plan-review plan-outlined plan-deleted job-added job-addendum job-blocked job-cancel-requested job-edited job-merged job-failed job-retried job-cancelled attempt-started attempt-retry attempt-status attempt-ended attempt-aborted attempt-nudge all-merged workflow-added workflow-base-changed workflow-described workflow-edited workflow-cancelled workflow-started pr-added pr-base-changed pr-cancelled pr-described pr-edited pr-feedback pr-opened pr-started pr-merged escalation escalation-answered escalation-expired review-opened review-closed session-registered session-state session-moved session-renamed session-archived session-asleep session-message session-resumed session-retiered inbox-opened inbox-closed inbox-claimed inbox-snoozed inbox-dismissed watch-started watch-stopped incident-started incident-ended incident-plans error".split(" ");
const INBOX_EVENT_KINDS = "inbox-opened inbox-closed inbox-claimed inbox-snoozed inbox-dismissed session-state session-message session-resumed plan-created plan-review job-failed job-blocked pr-opened pr-merged pr-feedback watch-started watch-stopped incident-started incident-ended session-registered session-moved".split(" ");

describe("cockpit service live updates", () => {
  it("every tree event kind refreshes the tree", async () => {
    expect(TREE_EVENT_KINDS).toHaveLength(72);
    const missing: string[] = [];
    for (const kind of TREE_EVENT_KINDS) {
      const { service, procs, scheduler, loads } = setup();
      await service.start();
      procs[0]!.emit(ev(41, kind));
      scheduler.advance(COALESCE_MS);
      await flush();
      if (!loads.some((l) => l.panes.includes("tree"))) missing.push(kind);
      service.stop();
    }
    expect(missing).toEqual([]);
  });

  it("every inbox event kind refreshes the inbox", async () => {
    expect(INBOX_EVENT_KINDS).toHaveLength(21);
    const missing: string[] = [];
    for (const kind of INBOX_EVENT_KINDS) {
      const { service, procs, scheduler, loads } = setup();
      await service.start();
      procs[0]!.emit(ev(41, kind));
      scheduler.advance(COALESCE_MS);
      await flush();
      if (!loads.some((l) => l.panes.includes("inbox"))) missing.push(kind);
      service.stop();
    }
    expect(missing).toEqual([]);
  });

  it("the safety refresh includes tree and inbox", async () => {
    const { service, scheduler, loads } = setup();
    await service.start();
    scheduler.advance(SAFETY_REFRESH_MS);
    await flush();
    expect(loads).toEqual([{ panes: ["inbox", "tree"], planId: null }]);
    service.stop();
  });

  it("overlapping refreshes are queued and none is dropped", async () => {
    const releases: (() => void)[] = [];
    const { service, procs, scheduler, loads } = setup(() => new Promise<void>((r) => { releases.push(r); }));
    await service.start();
    procs[0]!.emit(ev(41, "plan-done"));
    scheduler.advance(COALESCE_MS);
    await flush();
    expect(loads).toEqual([{ panes: ["tree"], planId: null }]);
    procs[0]!.emit(ev(42, "session-state"));
    scheduler.advance(COALESCE_MS);
    await flush();
    expect(loads).toHaveLength(1);
    releases[0]!();
    await flush();
    expect(loads).toEqual([{ panes: ["tree"], planId: null }, { panes: ["inbox", "tree"], planId: null }]);
    releases[1]!();
    await flush();
    service.stop();
  });

  it("a refresh runs right after an action completes", async () => {
    const { service, loads } = setup();
    await service.start();
    const out = await service.runAction({ kind: "inbox.dismiss", itemId: 7 });
    await flush();
    expect(out).toEqual({ ok: true });
    expect(loads).toEqual([{ panes: ["inbox", "tree"], planId: null }]);
    service.stop();
  });

  it("a claim refused as not_claimable rejects with the error and still refreshes the inbox", async () => {
    const refused: Cpd = { json: async () => { throw new Error("not_claimable: pull request acme/pay#5 is already approved; the item is closed."); } };
    const { service, loads } = setup(undefined, refused);
    await service.start();
    await expect(service.runAction({ kind: "inbox.claim", itemId: 7 })).rejects.toThrow(/not_claimable/);
    await flush();
    expect(loads).toEqual([{ panes: ["inbox", "tree"], planId: null }]);
    service.stop();
  });

  it("a plan.new action reply waits for an in-flight refresh to drain and for the tree to contain the new session", async () => {
    let created = false;
    const releases: (() => void)[] = [];
    const withPlan = (): ContextTreeModel => ({
      cursor: 41,
      contexts: [{ id: "c1", plans: [{ id: "my-plan", contextId: "c1", sessions: [{ ref: { id: "s-lead9" } }], leadSessionId: "s-lead9" }], sessions: [] }],
    } as unknown as ContextTreeModel);
    const readModel: ReadModel = {
      async loadAll() { return { tree: tree(40), plan: null, inbox }; },
      async loadPanes(panes: PaneSet) {
        const snapshot = created ? withPlan() : tree(41); // the read sees the tree as of its start
        await new Promise<void>((r) => { releases.push(r); });
        return panes.has("tree") ? { tree: snapshot } : {};
      },
    };
    const cpd: Cpd = { json: async () => { created = true; return { ok: true, plan: { id: "my-plan" }, lead: { session: "s-lead9" } }; } };
    const scheduler = new FakeScheduler();
    const procs: FakeFollow[] = [];
    const follow: FollowBoundary = { start: (args) => { const p = new FakeFollow(args); procs.push(p); return p; } };
    const service = createCockpitService({ readModel, cpd, follow, scheduler });
    await service.start();
    procs[0]!.emit(ev(41, "plan-done"));
    scheduler.advance(COALESCE_MS);
    await flush();
    expect(releases).toHaveLength(1); // a refresh is held open, with the pre-plan tree
    let reply: { hasNewSession: boolean } | null = null;
    const hasSession = () => JSON.stringify(service.snapshot()?.tree).includes("s-lead9");
    const done = service.runAction({ kind: "plan.new", planId: "my-plan", contextId: "c1", title: "My plan" }).then(() => { reply = { hasNewSession: hasSession() }; });
    await flush();
    expect(reply).toBeNull();
    releases[0]!();
    await flush();
    expect(reply).toBeNull(); // the queued read, which sees the new session, is still pending
    expect(releases).toHaveLength(2);
    releases[1]!();
    await flush();
    await done;
    expect(reply).toEqual({ hasNewSession: true });
    service.stop();
  });

  it("awaiting refresh while another refresh runs resolves after the queued panes are loaded", async () => {
    const releases: (() => void)[] = [];
    const { service, procs, scheduler, loads } = setup(() => new Promise<void>((r) => { releases.push(r); }));
    await service.start();
    procs[0]!.emit(ev(41, "plan-done"));
    scheduler.advance(COALESCE_MS);
    await flush();
    expect(loads).toEqual([{ panes: ["tree"], planId: null }]);
    let resolved = false;
    const done = service.selectPlan("p2").then(() => { resolved = true; }); // awaits refresh(plan) while one runs
    await flush();
    expect(resolved).toBe(false);
    releases[0]!();
    await flush();
    expect(loads).toEqual([{ panes: ["tree"], planId: null }, { panes: ["plan"], planId: "p2" }]);
    expect(resolved).toBe(false); // the queued plan load is still in flight
    releases[1]!();
    await flush();
    await done;
    expect(resolved).toBe(true);
    service.stop();
  });
});

describe("cockpit service live tree", () => {
  it("the tree pane advances while the cockpit stream is ready", async () => {
    const stream = new ReadyStream();
    const scheduler = new FakeScheduler();
    const procs: FakeFollow[] = [];
    const follow: FollowBoundary = { start: (args) => { const p = new FakeFollow(args); procs.push(p); return p; } };
    const readModel: ReadModel = {
      async loadAll() { return { tree: tree(0), plan: null, inbox }; },
      async loadPanes() { return {}; },
    };
    const service = createCockpitService({ readModel, cpd: { json: async () => ({ ok: true }) }, follow, scheduler, stream });
    const updates: CockpitUpdate[] = [];
    service.subscribe((u) => updates.push(u));
    await service.start();
    expect(procs).toHaveLength(0); // the stream is ready: no follow process runs

    // a committed batch ends in a Snapshot whose four cursors make a tree cursor of 20
    stream.snapshot(rawPanes([ctxRow("c1")], [], { context: 20, plan: 30, session: 25, inbox: 40 }), 1);
    scheduler.advance(COALESCE_MS);
    await flush();
    expect(updates.filter((u) => u.kind === "pane" && u.pane === "tree")).toHaveLength(1);

    // a later batch adds a Plan; its minimum cursor (10) is lower, which is not stale (14.1)
    stream.snapshot(rawPanes([ctxRow("c1")], [planRow("p2", "c1")], { context: 10, plan: 30, session: 25, inbox: 40 }), 2);
    scheduler.advance(COALESCE_MS);
    await flush();

    const trees = updates.filter((u) => u.kind === "pane" && u.pane === "tree");
    expect(trees).toHaveLength(2);
    const last = trees[1]!;
    const plans = last.kind === "pane" && last.pane === "tree" ? last.data.contexts.flatMap((c) => c.plans.map((p) => p.id)) : [];
    expect(plans).toEqual(["p2"]);
    service.stop();
  });
});

describe("cockpit service stale pane updates", () => {
  const planAt = (cursor: number): PlanViewModel => ({ planId: "p1", contextId: "c", title: "P", cursor } as unknown as PlanViewModel);
  const inboxAt = (cursor: number): InboxModel => ({ ...inbox, cursor });
  const stale = (update: PaneUpdate) => {
    const scheduler = new FakeScheduler();
    const procs: FakeFollow[] = [];
    const follow: FollowBoundary = { start: (args) => { const p = new FakeFollow(args); procs.push(p); return p; } };
    const readModel: ReadModel = {
      async loadAll() { return { tree: tree(40), plan: planAt(10), inbox: inboxAt(10) }; },
      async loadPanes() { return update; },
    };
    const service = createCockpitService({ readModel, cpd: { json: async () => ({ ok: true }) }, follow, scheduler });
    const updates: CockpitUpdate[] = [];
    service.subscribe((u) => updates.push(u));
    return { service, scheduler, procs, updates };
  };

  it("apply_drops_stale_inbox_update", async () => {
    const { service, procs, scheduler, updates } = stale({ inbox: inboxAt(5) });
    await service.start();
    procs[0]!.emit(ev(41, "inbox-opened"));
    scheduler.advance(COALESCE_MS);
    await flush();
    expect(service.snapshot().inbox?.cursor).toBe(10);
    expect(updates.filter((u) => u.kind === "pane" && u.pane === "inbox")).toEqual([]);
    service.stop();
  });

  it("apply_keeps_a_tree_update_with_a_lower_cursor", async () => {
    const { service, procs, scheduler, updates } = stale({ tree: tree(30) });
    await service.start();
    procs[0]!.emit(ev(41, "context-created"));
    scheduler.advance(COALESCE_MS);
    await flush();
    expect(service.snapshot().tree?.cursor).toBe(30);
    expect(updates.filter((u) => u.kind === "pane" && u.pane === "tree")).toHaveLength(1);
    service.stop();
  });

  it("apply_drops_stale_plan_update", async () => {
    const { service, updates } = stale({ plan: planAt(5) });
    await service.start();
    await service.selectPlan("p1");
    expect(service.snapshot().plan?.cursor).toBe(10);
    expect(updates.filter((u) => u.kind === "pane" && u.pane === "plan")).toEqual([]);
    service.stop();
  });
});
