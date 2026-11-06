import { describe, expect, it } from "vitest";
import type { PlanViewModel } from "../protocol/cockpit.ts";
import type { Completion, SessionRef, TranscriptItem } from "../protocol/domain.ts";
import type { SessionServerMessage } from "../protocol/wire.ts";
import type { CompletionPayload } from "./completion-classify.ts";
import { createCompletionService } from "./completion-service.ts";

const lead: SessionRef = { id: "s1", key: "c:p", runner: "pi", role: "lead", state: "idle", title: null };
const user = (id: string, text = id): TranscriptItem => ({ kind: "user", id, text, attachments: [] });
const done: Completion = { label: "safe_to_archive", text: "Safe to archive · 93%", score: 93, tone: "done" };
const plan = (cursor: number) => ({ planId: "p1", contextId: "c1", title: "P", phase: "execute", counts: {}, review: null, openEscalations: 0, workflows: [], jobs: [], cursor }) as unknown as PlanViewModel;

const flush = async (): Promise<void> => {
  for (let i = 0; i < 20; i++) await Promise.resolve();
};

const tail = (items: TranscriptItem[]): SessionServerMessage => ({ type: "session.window", sessionId: "s1", mode: "tail", window: { items, hasEarlier: false, cursor: null } });
const status = (busy: boolean): SessionServerMessage => ({ type: "session.status", sessionId: "s1", busy, queued: [] });

const setup = (opts: { ref?: SessionRef; results?: Array<Completion | null>; plans?: Array<PlanViewModel | null | Error>; manualClassify?: boolean } = {}) => {
  const payloads: CompletionPayload[] = [];
  const pending: Array<(c: Completion | null) => void> = [];
  const results = [...(opts.results ?? [])];
  const plans = [...(opts.plans ?? [])];
  let lastPlan: PlanViewModel | null | Error = plan(1);
  let planCalls = 0;
  const classifier = {
    classify: (p: CompletionPayload) => {
      payloads.push(p);
      if (opts.manualClassify === true) return new Promise<Completion | null>((r) => pending.push(r));
      return Promise.resolve(results.length > 0 ? (results.shift() ?? null) : done);
    },
  };
  const planOf = () => {
    planCalls += 1;
    if (plans.length > 0) {
      const next = plans.shift() ?? null;
      if (next instanceof Error) return Promise.reject(next);
      lastPlan = next;
    }
    return lastPlan instanceof Error ? Promise.reject(lastPlan) : Promise.resolve(lastPlan);
  };
  const sent: SessionServerMessage[] = [];
  const tracker = createCompletionService({ classifier, planOf }).track(opts.ref ?? lead, (m) => sent.push(m));
  return { tracker, sent, payloads, pending, planCalls: () => planCalls };
};
const multi = (results: Array<Completion | null> = []) => {
  const payloads: CompletionPayload[] = [];
  const queue = [...results];
  let planCalls = 0;
  const service = createCompletionService({
    classifier: { classify: (p: CompletionPayload) => { payloads.push(p); return Promise.resolve(queue.length > 0 ? (queue.shift() ?? null) : done); } },
    planOf: () => { planCalls += 1; return Promise.resolve(plan(1)); },
  });
  const track = (ref: SessionRef) => {
    const sent: SessionServerMessage[] = [];
    const tracker = service.track(ref, (m) => sent.push(m));
    return { tracker, sent };
  };
  return { track, payloads, planCalls: () => planCalls };
};
const completions = (sent: SessionServerMessage[]) => sent.filter((m) => m.type === "session.completion");

describe("completion service", () => {
  it("a tail window of a Lead schedules one classification and sends session.completion", async () => {
    const t = setup();
    t.tracker.observe(tail([user("u1")]));
    await flush();
    expect(t.payloads).toHaveLength(1);
    expect(t.payloads[0]?.session.id).toBe("s1");
    expect(t.payloads[0]?.plan?.id).toBe("p1");
    expect(t.sent).toEqual([{ type: "session.completion", sessionId: "s1", completion: done }]);
  });
  it("the same session item and plan cursor is served from the cache", async () => {
    const t = setup();
    t.tracker.observe(tail([user("u1")]));
    await flush();
    t.tracker.observe(tail([user("u1")]));
    await flush();
    expect(t.payloads).toHaveLength(1);
    expect(completions(t.sent)).toHaveLength(2);
  });
  it("a new last item recomputes", async () => {
    const t = setup();
    t.tracker.observe(tail([user("u1")]));
    await flush();
    t.tracker.observe(tail([user("u1"), user("u2")]));
    await flush();
    expect(t.payloads).toHaveLength(2);
    expect(completions(t.sent)).toHaveLength(2);
  });
  it("a new plan cursor recomputes", async () => {
    const t = setup({ plans: [plan(1), plan(2)] });
    t.tracker.observe(tail([user("u1")]));
    await flush();
    t.tracker.observe(tail([user("u1")]));
    await flush();
    expect(t.payloads).toHaveLength(2);
  });
  it("busy true to false schedules a classification", async () => {
    const t = setup();
    t.tracker.observe(tail([user("u1")]));
    await flush();
    t.tracker.observe(status(true));
    t.tracker.observe({ type: "session.append", sessionId: "s1", items: [user("u2")] });
    await flush();
    expect(t.payloads).toHaveLength(1);
    t.tracker.observe(status(false));
    await flush();
    expect(t.payloads).toHaveLength(2);
    expect(t.payloads[1]?.transcript.map((x) => x.text)).toEqual(["u1", "u2"]);
    expect(completions(t.sent)).toHaveLength(2);
  });
  it("busy staying true schedules nothing", async () => {
    const t = setup();
    t.tracker.observe(tail([user("u1")]));
    await flush();
    t.tracker.observe(status(true));
    t.tracker.observe(status(true));
    t.tracker.observe(status(false));
    t.tracker.observe(status(false));
    await flush();
    expect(t.payloads).toHaveLength(1); // the false after true hits the cache: same item, same cursor
    expect(completions(t.sent)).toHaveLength(2);
    t.tracker.observe(status(true));
    t.tracker.observe(status(true));
    await flush();
    expect(completions(t.sent)).toHaveLength(2);
  });
  it("a manager session is never classified", async () => {
    const t = setup({ ref: { ...lead, role: "manager" } });
    t.tracker.observe(tail([user("u1")]));
    t.tracker.observe(status(true));
    t.tracker.observe(status(false));
    await flush();
    expect(t.payloads).toHaveLength(0);
    expect(t.planCalls()).toBe(0);
    expect(t.sent).toEqual([]);
    const control = setup();
    control.tracker.observe(tail([user("u1")]));
    await flush();
    expect(control.payloads).toHaveLength(1); // control: the same messages for a Lead classify
  });
  it("an empty transcript is never classified", async () => {
    const t = setup();
    t.tracker.observe(tail([]));
    t.tracker.observe(status(true));
    t.tracker.observe(status(false));
    await flush();
    expect(t.payloads).toHaveLength(0);
    expect(t.sent).toEqual([]);
    t.tracker.observe(tail([user("u1")])); // control: a non-empty transcript classifies
    await flush();
    expect(t.payloads).toHaveLength(1);
  });
  it("track replays the last known completion of the session at once", async () => {
    const m = multi();
    const a = m.track(lead);
    a.tracker.observe(tail([user("u1")]));
    await flush();
    const callsBefore = m.planCalls();
    const b = m.track(lead);
    expect(completions(b.sent)).toEqual([{ type: "session.completion", sessionId: "s1", completion: done }]);
    await flush();
    expect(completions(b.sent)).toHaveLength(1);
    expect(m.planCalls()).toBe(callsBefore);
  });
  it("a null classifier result sends nothing and keeps the last completion", async () => {
    const m = multi([done, null]);
    const a = m.track(lead);
    a.tracker.observe(tail([user("u1")]));
    await flush();
    expect(a.sent).toEqual([{ type: "session.completion", sessionId: "s1", completion: done }]);
    a.tracker.observe(tail([user("u1"), user("u2")]));
    await flush();
    expect(m.payloads).toHaveLength(2);
    expect(a.sent).toHaveLength(1);
    expect(a.sent.some((x) => x.type === "session.completion" && x.completion === null)).toBe(false);
    expect(m.track(lead).sent).toEqual([{ type: "session.completion", sessionId: "s1", completion: done }]);
    const fresh = multi([null]);
    const f = fresh.track(lead);
    f.tracker.observe(tail([user("u1")]));
    await flush();
    expect(fresh.payloads).toHaveLength(1);
    expect(f.sent).toEqual([]);
  });
  it("the last completion is kept per session and never replayed to a manager", async () => {
    const m = multi();
    const a = m.track(lead);
    a.tracker.observe(tail([user("u1")]));
    await flush();
    expect(completions(m.track(lead).sent)).toHaveLength(1); // control: the same session replays
    expect(m.track({ ...lead, id: "s2" }).sent).toEqual([]);
    expect(m.track({ ...lead, role: "manager" }).sent).toEqual([]);
  });
  it("concurrent computations of one key share one classification", async () => {
    const t = setup({ manualClassify: true });
    t.tracker.observe(tail([user("u1")]));
    await flush();
    t.tracker.observe(tail([user("u1")]));
    await flush();
    expect(t.payloads).toHaveLength(1);
    t.pending[0]?.(done);
    await flush();
    expect(completions(t.sent).length).toBeGreaterThanOrEqual(1);
    expect(completions(t.sent).every((m) => m.type === "session.completion" && m.completion === done)).toBe(true);
    expect(t.payloads).toHaveLength(1);
  });
  it("a disposed tracker sends nothing", async () => {
    const t = setup({ manualClassify: true });
    t.tracker.observe(tail([user("u1")]));
    await flush();
    t.tracker.dispose();
    t.pending[0]?.(done);
    await flush();
    expect(t.sent).toEqual([]);
    t.tracker.observe(tail([user("u1")]));
    await flush();
    expect(t.payloads).toHaveLength(1);
    expect(t.sent).toEqual([]);
  });
  it("a planOf rejection sends nothing", async () => {
    const t = setup({ plans: [new Error("cpd down")] });
    t.tracker.observe(tail([user("u1")]));
    await flush();
    expect(t.payloads).toHaveLength(0);
    expect(t.sent).toEqual([]);
    t.tracker.observe(tail([user("u1")])); // control: the next computation recovers
    await flush();
    expect(t.payloads).toHaveLength(1);
    expect(t.sent).toEqual([{ type: "session.completion", sessionId: "s1", completion: done }]);
  });
  it("a null plan sends nothing", async () => {
    const t = setup({ plans: [null] });
    t.tracker.observe(tail([user("u1")]));
    await flush();
    expect(t.payloads).toHaveLength(0);
    expect(t.sent).toEqual([]);
  });
  it("a pending classification does not block observe", async () => {
    const t = setup({ manualClassify: true });
    t.tracker.observe(tail([user("u1")]));
    await flush();
    expect(t.payloads).toHaveLength(1);
    expect(() => {
      t.tracker.observe({ type: "session.append", sessionId: "s1", items: [user("u2")] });
      t.tracker.observe(status(true));
      t.tracker.observe(status(false));
    }).not.toThrow();
    await flush();
    expect(t.sent).toEqual([]); // nothing resolved yet, observe returned regardless
    expect(t.payloads).toHaveLength(2); // the stale in-flight key is not cancelled; a new key starts
  });
  it("session.append and session.update keep the mirrored list", async () => {
    const t = setup();
    t.tracker.observe(tail([user("u2", "two")]));
    await flush();
    t.tracker.observe({ type: "session.window", sessionId: "s1", mode: "earlier", window: { items: [user("u1", "one")], hasEarlier: false, cursor: null } });
    t.tracker.observe({ type: "session.append", sessionId: "s1", items: [user("u3", "three")] });
    t.tracker.observe({ type: "session.update", sessionId: "s1", items: [user("u2", "two edited")] });
    t.tracker.observe(status(true));
    t.tracker.observe(status(false));
    await flush();
    const last = t.payloads[t.payloads.length - 1];
    expect(last?.transcript.map((x) => x.text)).toEqual(["one", "two edited", "three"]);
  });
  it("an open dialog is reported in the payload", async () => {
    const t = setup();
    t.tracker.observe(tail([user("u1")]));
    await flush();
    t.tracker.observe({ type: "session.dialog", sessionId: "s1", dialog: { id: "d1", interactive: true, kind: "confirm", title: "t", message: "m" } as never });
    t.tracker.observe({ type: "session.append", sessionId: "s1", items: [user("u2")] });
    t.tracker.observe(status(true));
    t.tracker.observe(status(false));
    await flush();
    expect(t.payloads[t.payloads.length - 1]?.session.hasOpenDialog).toBe(true);
    t.tracker.observe({ type: "session.dialogResolved", sessionId: "s1", dialogId: "d1", outcome: "answered" });
    t.tracker.observe({ type: "session.append", sessionId: "s1", items: [user("u3")] });
    t.tracker.observe(status(true));
    t.tracker.observe(status(false));
    await flush();
    expect(t.payloads[t.payloads.length - 1]?.session.hasOpenDialog).toBe(false);
  });
});
