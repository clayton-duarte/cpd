import { describe, expect, it } from "vitest";
import { FakeScheduler } from "../../testing/scheduler.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import type { ContextTreeModel, InboxModel, PlanViewModel } from "../../protocol/cockpit.ts";
import type { Cpd, FollowBoundary, FollowProcess } from "./cli.ts";
import type { PaneSet, ReadModel } from "./readmodel.ts";
import { COALESCE_MS, SAFETY_REFRESH_MS, FOLLOW_RESTART_BASE_MS, createCockpitService, type CockpitUpdate } from "./events.ts";

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
});
