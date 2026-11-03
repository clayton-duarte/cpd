import { describe, expect, it } from "vitest";
import { FakeScheduler } from "../../testing/scheduler.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import type { ContextTreeModel, InboxModel, PlanViewModel } from "../../protocol/cockpit.ts";
import type { Cpd, FollowBoundary, FollowProcess } from "./cli.ts";
import type { PaneSet, ReadModel } from "./readmodel.ts";
import { COALESCE_MS, FOLLOW_RESTART_BASE_MS, createCockpitService, type CockpitUpdate } from "./events.ts";

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

function setup() {
  const scheduler = new FakeScheduler();
  const procs: FakeFollow[] = [];
  const follow: FollowBoundary = { start: (args) => { const p = new FakeFollow(args); procs.push(p); return p; } };
  const loads: { panes: string[]; planId: string | null }[] = [];
  let all = 0;
  const readModel: ReadModel = {
    async loadAll() { all++; return { tree: tree(40), plan: null, inbox }; },
    async loadPanes(panes: PaneSet, planId) {
      loads.push({ panes: [...panes].sort(), planId });
      const out: { tree?: ContextTreeModel; plan?: PlanViewModel | null; inbox?: InboxModel } = {};
      if (panes.has("tree")) out.tree = tree(41);
      if (panes.has("inbox")) out.inbox = inbox;
      if (panes.has("plan")) out.plan = null;
      return out;
    },
  };
  const cpd: Cpd = { json: async () => ({ ok: true }) };
  const service = createCockpitService({ readModel, cpd, follow, scheduler });
  const updates: CockpitUpdate[] = [];
  service.subscribe((u) => updates.push(u));
  return { service, scheduler, procs, loads, updates, allCount: () => all };
}

const ev = (id: number, kind: string) => JSON.stringify({ id, kind, plan: null, context: null, session: null, job: null, attempt: null, at: "2026-01-01T00:00:00Z", text: "" });
const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };

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
});
