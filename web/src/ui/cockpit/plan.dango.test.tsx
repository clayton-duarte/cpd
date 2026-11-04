// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import type { JobView, PlanViewModel } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { glyphClass } from "../dango-status.ts";
import { PlanView } from "./PlanView.tsx";

function job(key: string, status: string, needs: string[], workflow: string | null, extra: Partial<JobView> = {}): JobView {
  return { key, title: `T ${key}`, kind: "work", status, tier: "S1", needs, workflow, attempts: [], ...extra } as JobView;
}

function plan(phase: string, jobs: JobView[], wfJobKeys: string[] = ["job-1", "job-2", "job-3"]): PlanViewModel {
  return {
    planId: "p1",
    contextId: "c1",
    title: "Web UI",
    phase,
    counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 },
    review: null,
    openEscalations: 0,
    cursor: 1,
    workflows: [
      {
        key: "wf-a",
        title: "A",
        repo: "cpd",
        base: "main",
        status: "open",
        needs: [],
        jobKeys: wfJobKeys,
        pr: { number: 7, url: "https://example.test/pr/7", state: "open", checks: null, mergeable: null },
      },
    ],
    jobs,
  } as PlanViewModel;
}

const chain = [
  job("job-1", "merged", [], "wf-a"),
  job("job-2", "running", ["job-1"], "wf-a"),
  job("job-3", "queued", ["job-2"], "wf-a"),
];

it("plan view renders the phase stepper with the current phase marked", () => {
  const m = mount(<PlanView plan={plan("dispatch", chain)} onAction={vi.fn()} />);
  const steps = Array.from(m.container.querySelectorAll('ol[aria-label="Phase"] li[data-phase]'));
  expect(steps.map((s) => s.getAttribute("data-phase"))).toEqual(["grounding", "plan", "dispatch", "deploy", "done"]);
  expect(steps.map((s) => s.textContent)).toEqual(["grounding", "planning", "dispatch", "deploy", "done"]);
  expect(steps.map((s) => s.getAttribute("data-state"))).toEqual(["past", "past", "current", "upcoming", "upcoming"]);
  const current = m.container.querySelectorAll('ol[aria-label="Phase"] [aria-current="step"]');
  expect(current).toHaveLength(1);
  expect(current[0]?.getAttribute("data-phase")).toBe("dispatch");
  expect(m.container.querySelectorAll('ol[aria-label="Phase"] li')).toHaveLength(9);
  m.unmount();
});

it("plan view renders a workflow card with a node per job", () => {
  const m = mount(<PlanView plan={plan("dispatch", chain)} onAction={vi.fn()} />);
  const card = m.container.querySelector('section[data-workflow="wf-a"]');
  expect(card?.getAttribute("data-status")).toBe("open");
  expect(card?.querySelector('[data-part="wf-header"] h3')?.textContent).toBe("wf-a");
  expect(card?.querySelector('[data-part="wf-route"]')?.textContent).toBe("cpd → main");
  const nodes = card?.querySelectorAll('[data-part="dag"] > [data-job]');
  expect(Array.from(nodes ?? []).map((n) => n.getAttribute("data-job"))).toEqual(["job-1", "job-2", "job-3"]);
  m.unmount();
});

it("plan view nodes carry status glyph, key, tier and subtitle", () => {
  const jobs = [
    job("job-1", "merged", [], "wf-a", { tier: "S2" }),
    job("job-2", "failed", ["job-1"], "wf-a", {
      attempts: [
        { id: "a1", number: 1, tier: "S1", status: "failed", outcome: null, cause: null, workerSession: null },
        { id: "a2", number: 2, tier: "S1", status: "failed", outcome: null, cause: null, workerSession: null },
      ],
    }),
    job("job-3", "queued", ["job-2", "job-9"], "wf-a"),
  ];
  const m = mount(<PlanView plan={plan("dispatch", jobs)} onAction={vi.fn()} />);
  const node = (k: string): Element | null => m.container.querySelector(`[data-part="dag"] [data-job="${k}"]`);
  const one = node("job-1");
  expect(one?.querySelector('[data-part="name"]')?.textContent).toBe("job-1");
  expect(one?.querySelector('[data-part="tier"]')?.textContent).toBe("S2");
  expect(one?.querySelector('[data-part="status"]')?.textContent).toBe("merged");
  expect(one?.querySelector(`[aria-hidden="true"][class="${glyphClass.done}"]`)).not.toBeNull();
  expect(node("job-2")?.querySelector('[data-part="status"]')?.textContent).toBe("failed ×2");
  expect(node("job-2")?.querySelector(`[class="${glyphClass.blocked}"]`)).not.toBeNull();
  expect(node("job-3")?.querySelector('[data-part="status"]')?.textContent).toBe("blocked on 2");
  m.unmount();
});

it("plan view connects dependent jobs with svg edges", () => {
  const m = mount(<PlanView plan={plan("dispatch", chain)} onAction={vi.fn()} />);
  const svg = m.container.querySelector('svg[data-part="dag-edges"]');
  expect(svg?.getAttribute("aria-hidden")).toBe("true");
  const edges = Array.from(svg?.querySelectorAll("path[data-edge]") ?? []);
  expect(edges.map((e) => e.getAttribute("data-edge"))).toEqual(["job-1>job-2", "job-2>job-3"]);
  expect(edges.map((e) => e.getAttribute("data-state"))).toEqual(["done", "pending"]);
  expect(edges.map((e) => e.getAttribute("d"))).toEqual([
    "M149 23H173M169 19.5L173 23 169 26.5",
    "M325 23H349M345 19.5L349 23 345 26.5",
  ]);
  m.unmount();
});

it("plan view shows Retry on a failed job and dispatches its action", () => {
  const jobs = [job("job-1", "merged", [], "wf-a"), job("job-2", "failed", ["job-1"], "wf-a"), job("job-3", "queued", [], "wf-a")];
  const onAction = vi.fn();
  const onRetryJob = vi.fn();
  const m = mount(<PlanView plan={plan("dispatch", jobs)} onAction={onAction} onRetryJob={onRetryJob} />);
  const buttons = m.container.querySelectorAll('button[data-action="retry"]');
  expect(buttons).toHaveLength(1);
  const btn = m.container.querySelector('[data-job="job-2"] button[data-action="retry"]') as HTMLButtonElement | null;
  expect(btn?.textContent).toBe("Retry ▸");
  expect(btn?.getAttribute("aria-label")).toBe("Retry job-2");
  btn?.click();
  expect(onRetryJob).toHaveBeenCalledExactlyOnceWith("job-2", "wf-a");
  expect(onAction).not.toHaveBeenCalled();
  m.unmount();
  const bare = mount(<PlanView plan={plan("dispatch", jobs)} onAction={onAction} />);
  expect(bare.container.querySelector('button[data-action="retry"]')).toBeNull();
  bare.unmount();
});

it("plan view lists explore jobs as chips", () => {
  const jobs = [
    ...chain,
    job("x1", "running", [], null, { kind: "scout" }),
    job("x2", "queued", [], null, { kind: "research" }),
  ];
  const m = mount(<PlanView plan={plan("dispatch", jobs)} onAction={vi.fn()} />);
  const strip = m.container.querySelector('section[aria-label="Explore jobs"][data-part="explore"]');
  expect(strip?.querySelector('[data-part="explore-summary"]')?.textContent).toBe("explore · 1 scout · 1 research");
  const chips = Array.from(strip?.querySelectorAll('[data-part="chips"] > li[data-job]') ?? []);
  expect(chips.map((c) => c.getAttribute("data-job"))).toEqual(["x1", "x2"]);
  expect(chips.map((c) => c.getAttribute("data-kind"))).toEqual(["scout", "research"]);
  expect(chips[0]?.querySelector('[data-part="name"]')?.textContent).toBe("x1");
  m.unmount();
  const none = mount(<PlanView plan={plan("dispatch", chain)} onAction={vi.fn()} />);
  expect(none.container.querySelector('[data-part="explore"]')).toBeNull();
  expect(none.container.querySelector('section[aria-label="Explore jobs"]')).toBeNull();
  none.unmount();
});

it("plan view keeps the post pr action", () => {
  const onAction = vi.fn();
  const m = mount(<PlanView plan={plan("dispatch", chain)} onAction={onAction} />);
  const pr = m.container.querySelector('[data-part="pr"]');
  expect(pr?.querySelector('a[href="https://example.test/pr/7"]')?.textContent).toBe("#7 open");
  const btn = pr?.querySelector('button[data-action="post-pr"]') as HTMLButtonElement | null;
  expect(btn?.textContent).toBe("Post PR");
  btn?.click();
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ kind: "pr.post", planId: "p1", workflow: "wf-a" });
  m.unmount();
});
