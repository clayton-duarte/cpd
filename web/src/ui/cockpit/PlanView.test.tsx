// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PlanViewModel } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { PlanView } from "./PlanView.tsx";

const plan: PlanViewModel = {
  planId: "p1",
  contextId: "c1",
  title: "Web UI",
  phase: "dispatch",
  counts: { draft: 0, queued: 1, running: 1, merged: 0, failed: 0, cancelled: 0 },
  review: { id: "r1", status: "approved", current: true },
  openEscalations: 0,
  cursor: 3,
  workflows: [
    {
      key: "wf-a",
      title: "A",
      repo: "cpd",
      base: "main",
      status: "open",
      needs: [],
      jobKeys: ["job-1", "job-2"],
      pr: { number: 7, url: "https://example.test/pr/7", state: "open", checks: null, mergeable: null },
    },
  ],
  jobs: [
    { key: "job-1", title: "First", kind: "work", status: "merged", tier: "S1", needs: [], workflow: "wf-a", attempts: [] },
    {
      key: "job-2",
      title: "Second",
      kind: "work",
      status: "running",
      tier: "S2",
      needs: ["job-1", "job-0"],
      workflow: "wf-a",
      attempts: [
        { id: "a1", number: 1, tier: "S2", status: "running", outcome: null, cause: null, workerSession: null },
      ],
    },
    { key: "job-x", title: "Explore", kind: "explore", status: "queued", tier: "S1", needs: [], workflow: null, attempts: [] },
  ],
};

it("renders workflows with their jobs and statuses", () => {
  const onAction = vi.fn();
  const m = mount(<PlanView plan={plan} onAction={onAction} />);
  const key = m.container.querySelector('section[aria-label="Plan p1"] h2[data-part="plan-key"]');
  expect(key?.getAttribute("title")).toBe("Web UI");
  expect(key?.textContent).toBe("p1");
  expect(m.container.textContent).toContain("Review r1: approved");
  const wf = m.container.querySelector('section[data-workflow="wf-a"]');
  expect(wf?.textContent).toContain("main");
  expect(wf?.querySelector('a[href="https://example.test/pr/7"]')?.textContent).toContain("#7 open");
  const jobs = wf?.querySelectorAll("li[data-job]");
  expect(Array.from(jobs ?? []).map((j) => j.getAttribute("data-job"))).toEqual(["job-1", "job-2"]);
  expect(wf?.querySelector('li[data-job="job-2"]')?.getAttribute("data-status")).toBe("running");
  expect(wf?.querySelector('li[data-job="job-2"]')?.textContent).toContain("#1 running");
  const explore = m.container.querySelector('section[aria-label="Explore jobs"]');
  expect(explore?.querySelector('li[data-job="job-x"]')).not.toBeNull();
  m.unmount();
});

it("shows job needs", () => {
  const m = mount(<PlanView plan={plan} onAction={vi.fn()} />);
  expect(m.container.querySelector('li[data-job="job-2"]')?.textContent).toContain("needs: job-1, job-0");
  expect(m.container.querySelector('li[data-job="job-1"]')?.textContent).not.toContain("needs:");
  m.unmount();
});

describe("PlanView with a flowchart", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
  });
  afterEach(() => { vi.unstubAllGlobals(); });

  it("PlanView renders the flowchart canvas when plan is null", () => {
    const m = mount(<PlanView plan={null} onAction={vi.fn()} />);
    const canvas = m.container.querySelector('[data-part="flow-canvas"]');
    expect(canvas).not.toBeNull();
    expect(canvas?.querySelectorAll("li[data-step]").length).toBe(20);
    expect(m.container.querySelector("[data-empty]")).toBeNull();
    expect(m.container.querySelector('section[aria-label^="Plan "]')).toBeNull();
    m.unmount();
  });

  it("the workflow dag-body scrolls with a ScrollArea", () => {
    const m = mount(<PlanView plan={plan} onAction={vi.fn()} />);
    const body = m.container.querySelector('[data-part="dag-body"]');
    expect(body?.getAttribute("data-slot")).toBe("scroll-area");
    expect(body?.className).not.toContain("overflow-x-auto");
    expect(body?.querySelector('[data-part="dag-canvas"]')).not.toBeNull();
    m.unmount();
  });
});

const row = (c: HTMLElement) => c.querySelector<HTMLElement>('[data-part="title-row"]');

it("PlanView header shows manager key, a slash, the plan key and the phase in a TopPaneHeader", () => {
  const m = mount(<PlanView plan={plan} onAction={vi.fn()} onSelectManager={vi.fn()} />);
  const h = row(m.container)!;
  expect(h.tagName).toBe("HEADER");
  for (const c of ["h-11"]) expect(h.classList.contains(c)).toBe(true);
  const key = h.querySelector('h2[data-part="plan-key"]')!;
  expect(key.textContent).toBe("p1");
  expect(key.getAttribute("title")).toBe("Web UI");
  expect(key.classList.contains("truncate")).toBe(true);
  const mgr = h.querySelector('[data-action="select-manager"]')!;
  const stepper = h.querySelector('[data-part="stepper"]')!;
  expect(stepper.getAttribute("data-current")).toBe("dispatch");
  const follows = (a: Element, b: Element) => Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
  expect(follows(mgr, key)).toBe(true);
  expect(follows(key, stepper)).toBe(true);
  expect(h.textContent).toContain("/");
  m.unmount();
});

it("PlanView header sits outside the scroll area above the scrolling body", () => {
  const m = mount(<PlanView plan={plan} onAction={vi.fn()} onSelectManager={vi.fn()} />);
  const header = row(m.container)!;
  const section = header.parentElement!;
  expect(section.tagName).toBe("SECTION");
  for (const c of ["flex", "h-full", "min-h-0", "w-full", "min-w-0", "flex-col", "overflow-hidden"]) expect(section.classList.contains(c)).toBe(true);
  expect(section.firstElementChild).toBe(header);
  expect(header.closest("[data-slot=scroll-area]")).toBeNull();
  const scroll = section.querySelector<HTMLElement>('[data-part="plan-scroll"]')!;
  expect(scroll.getAttribute("data-slot")).toBe("scroll-area");
  expect(scroll.parentElement).toBe(section);
  for (const c of ["min-h-0", "flex-1"]) expect(scroll.classList.contains(c)).toBe(true);
  expect(scroll.querySelector('[data-part="plan-body"]')).not.toBeNull();
  expect(header.classList.contains("sticky")).toBe(false);
  m.unmount();
});

it("PlanView manager key is a button that calls onSelectManager", () => {
  const onSelectManager = vi.fn();
  const m = mount(<PlanView plan={plan} onAction={vi.fn()} onSelectManager={onSelectManager} />);
  const b = row(m.container)?.querySelector<HTMLElement>('button[data-action="select-manager"]');
  expect(b).not.toBeNull();
  expect(b!.textContent).toBe("c1");
  act(() => { b!.click(); });
  expect(onSelectManager).toHaveBeenCalledTimes(1);
  m.unmount();
});

it("PlanView manager key is plain text without onSelectManager", () => {
  const m = mount(<PlanView plan={plan} onAction={vi.fn()} />);
  const h = row(m.container)!;
  expect(h.querySelector('[data-action="select-manager"]')).toBeNull();
  expect(h.querySelector("button")).toBeNull();
  expect(h.querySelector('span[data-part="manager-key"]')?.textContent).toBe("c1");
  m.unmount();
});

it("PlanView keeps the review and escalation badges in the header actions", () => {
  const m = mount(<PlanView plan={{ ...plan, openEscalations: 2 }} onAction={vi.fn()} />);
  const h = row(m.container)!;
  expect(h.tagName).toBe("HEADER");
  expect(h.classList.contains("h-11")).toBe(true);
  const kids = Array.from(h.children);
  const spacer = kids.findIndex((k) => k.classList.contains("flex-1"));
  expect(spacer).toBeGreaterThan(0);
  const after = kids.slice(spacer + 1);
  expect(after.find((k) => k.getAttribute("data-part") === "review")?.textContent).toBe("Review r1: approved");
  expect(after.find((k) => k.getAttribute("data-part") === "escalations")?.textContent).toBe("Escalations: 2");
  m.unmount();
});

it("PlanView body is a plan-body section below the header", () => {
  const m = mount(<PlanView plan={plan} onAction={vi.fn()} />);
  const section = m.container.querySelector<HTMLElement>('section[aria-label="Plan p1"]')!;
  for (const c of ["flex", "min-w-0", "flex-col", "overflow-x-hidden"]) expect(section.classList.contains(c)).toBe(true);
  const body = section.querySelector<HTMLElement>('div[data-part="plan-body"]')!;
  expect(body).not.toBeNull();
  for (const c of ["flex", "flex-col", "gap-2.5", "px-4", "py-2.5"]) expect(body.classList.contains(c)).toBe(true);
  expect(body.querySelector('section[aria-label="Explore jobs"]')).not.toBeNull();
  expect(body.querySelector('section[data-workflow="wf-a"]')).not.toBeNull();
  expect(row(m.container)!.parentElement).toBe(section);
  expect(body.contains(row(m.container))).toBe(false);
  expect(row(m.container)!.compareDocumentPosition(body) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  m.unmount();
});
