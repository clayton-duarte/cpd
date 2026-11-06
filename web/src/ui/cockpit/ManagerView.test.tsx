// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { ContextNode, PlanNode, PlanWorkflowSummary, SessionNode } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { Dot } from "./ContextTree.tsx";
import { ManagerView } from "./ManagerView.tsx";

// jsdom has no ResizeObserver, which the flow canvas (external) needs: an inert double.
vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

const counts = { draft: 1, queued: 1, running: 0, merged: 3, failed: 0, cancelled: 0 };
const idle = { state: "idle", attention: null } as const;

function lead(id: string, dot: SessionNode["dot"] = idle): SessionNode {
  return {
    ref: { id, key: `ctx:${id}`, runner: "durable", role: "lead", state: dot.state, title: id },
    contextId: "ctx", planId: id, jobKey: null, archived: false, dot,
  };
}
function pr(n: number, over: Partial<NonNullable<PlanWorkflowSummary["pr"]>> = {}): PlanWorkflowSummary["pr"] {
  return { number: n, url: `https://example.test/pull/${n}`, state: "open", checks: "passing", mergeable: null, ...over };
}
function wf(key: string, n: number | null, status = "open"): PlanWorkflowSummary {
  return { key, status, repo: "r", pr: n === null ? null : pr(n) };
}
function plan(id: string, over: Partial<PlanNode> = {}): PlanNode {
  const l = lead(`lead-${id}`);
  return {
    id, contextId: "ctx", title: `Title of ${id}`, phase: "dispatch", archived: false, counts,
    leadSessionId: l.ref.id, sessions: [l], createdAt: "2026-01-01T00:00:00Z", workflows: [], ...over,
  };
}
function ctx(plans: PlanNode[], over: Partial<ContextNode> = {}): ContextNode {
  return { id: "ctx", title: "Context One", archived: false, records: 4, openQuestions: 0, sessions: [], plans, ...over };
}

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });

function show(context: ContextNode | null, onSelectPlan = vi.fn(), onSelectSession = vi.fn()) {
  const view = mount(<ManagerView context={context} onSelectPlan={onSelectPlan} onSelectSession={onSelectSession} />);
  mounted = view;
  return { container: view.container, onSelectPlan, onSelectSession };
}
const q = (c: ParentNode, sel: string) => c.querySelector<HTMLElement>(sel);
const qa = (c: ParentNode, sel: string) => [...c.querySelectorAll<HTMLElement>(sel)];
const row = (c: ParentNode, id: string) => q(c, `li[data-manager-plan="${id}"]`)!;

it("ManagerView renders the flowchart canvas when there is no context", () => {
  const { container } = show(null);
  expect(q(container, '[data-part="flow-canvas"]')).not.toBeNull();
});

it("ManagerView header is a TopPaneHeader with the manager key and counters", () => {
  const { container } = show(ctx([plan("a"), plan("b", { phase: "done" })]));
  const header = q(container, '[data-part="manager-header"]')!;
  expect(header).not.toBeNull();
  expect(header.classList.contains("h-11")).toBe(true);
  expect(q(header, '[data-part="manager-key"]')!.textContent).toBe("ctx");
  expect(q(header, '[data-part="manager-counters"]')!.textContent).toBe("1 active · 1 done · 4 records");
  expect(header.querySelector("button")).toBeNull();
  expect(header.tagName).toBe("HEADER");
  const kids = Array.from(header.children);
  const spacer = kids.findIndex((k) => k.classList.contains("flex-1"));
  expect(spacer).toBeGreaterThan(0);
  expect(kids.slice(0, spacer).some((k) => k.getAttribute("data-part") === "manager-key")).toBe(true);
});

it("ManagerView header sits outside the scroll area above the scrolling body", () => {
  const { container } = show(ctx([plan("a")]));
  const header = q(container, '[data-part="manager-header"]')!;
  const section = header.parentElement!;
  expect(section.tagName).toBe("SECTION");
  for (const c of ["flex", "h-full", "min-h-0", "w-full", "min-w-0", "flex-col", "overflow-hidden"]) expect(section.classList.contains(c)).toBe(true);
  expect(section.firstElementChild).toBe(header);
  expect(header.closest("[data-slot=scroll-area]")).toBeNull();
  const scroll = q(section, '[data-part="plan-scroll"]')!;
  expect(scroll.getAttribute("data-slot")).toBe("scroll-area");
  expect(scroll.parentElement).toBe(section);
  for (const c of ["min-h-0", "flex-1"]) expect(scroll.classList.contains(c)).toBe(true);
  expect(scroll.querySelector('[data-part="manager-body"]')).not.toBeNull();
  expect(Boolean(header.compareDocumentPosition(scroll) & Node.DOCUMENT_POSITION_FOLLOWING)).toBe(true);
  expect(header.classList.contains("sticky")).toBe(false);
});

it("ManagerView orders each plan row as dot, key, title, PRs, phase, actor, counters", () => {
  const { container } = show(ctx([plan("a", { workflows: [wf("w1", 1)] })]));
  const r = row(container, "a");
  const dot = q(r, "[data-dot]")!;
  const parts = ["plan-id", "plan-title", "plan-workflows", "plan-phase", "plan-actor", "plan-counters"]
    .map((p) => q(r, `[data-part="${p}"]`));
  for (const p of parts) expect(p).not.toBeNull();
  const seq = [dot, ...parts] as HTMLElement[];
  for (let i = 0; i + 1 < seq.length; i++) {
    expect(seq[i]!.compareDocumentPosition(seq[i + 1]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  }
});

it("ManagerView title is the flexible truncating item", () => {
  const { container } = show(ctx([plan("a")]));
  const t = q(row(container, "a"), '[data-part="plan-title"]')!;
  expect(t).not.toBeNull();
  for (const c of ["min-w-0", "flex-1", "truncate"]) expect(t.classList.contains(c)).toBe(true);
});

it("ManagerView shows up to two PR chips inline as external links", () => {
  const { container } = show(ctx([plan("a", { workflows: [wf("w1", 11), wf("w2", 12)] })]));
  const chips = qa(row(container, "a"), 'a[data-part="wf-pr-chip"]');
  expect(chips.map((c) => c.getAttribute("href"))).toEqual(["https://example.test/pull/11", "https://example.test/pull/12"]);
  for (const c of chips) expect(c.getAttribute("target")).toBe("_blank");
  expect(q(row(container, "a"), '[data-part="wf-pr-summary"]')).toBeNull();
});

it("ManagerView collapses more than two PRs into one hover trigger listing every chip", () => {
  const wfs = [wf("w1", 11), wf("w2", 12), wf("w3", 13)];
  const { container } = show(ctx([plan("a", { workflows: wfs })]));
  const r = row(container, "a");
  expect(qa(r, 'a[data-part="wf-pr-chip"]')).toHaveLength(0);
  const trigger = qa(r, 'button[data-part="wf-pr-summary"]');
  expect(trigger).toHaveLength(1);
  expect(trigger[0]!.textContent).toContain("3 PRs");
  // Radix HoverCard opens on focus; its content is portalled into document.body.
  act(() => { trigger[0]!.focus(); });
  const list = q(document.body, '[data-part="wf-pr-hover-list"]');
  expect(list).not.toBeNull();
  expect(qa(list!, 'a[data-part="wf-pr-chip"]')).toHaveLength(3);
});

it("ManagerView omits cancelled workflows", () => {
  const { container } = show(ctx([plan("a", { workflows: [wf("w1", 11), wf("w2", 12, "cancelled"), wf("w3", 13, "cancelled")] })]));
  const r = row(container, "a");
  expect(qa(r, 'a[data-part="wf-pr-chip"]').map((c) => c.getAttribute("href"))).toEqual(["https://example.test/pull/11"]);
  expect(q(r, '[data-part="wf-pr-summary"]')).toBeNull();
});

it("ManagerView lists done plans in a collapsed section", () => {
  const { container } = show(ctx([plan("a"), plan("d1", { phase: "done" }), plan("d2", { phase: "done" })]));
  expect(q(container, 'li[data-manager-plan="a"]')).not.toBeNull();
  expect(q(container, 'li[data-manager-plan="d1"]')).toBeNull();
  const trigger = qa(container, "button").find((b) => b.textContent === "Done plans (2)")!;
  expect(trigger).toBeDefined();
  act(() => { trigger.click(); });
  expect(q(container, 'li[data-manager-plan="d1"]')).not.toBeNull();
  expect(q(container, 'li[data-manager-plan="d2"]')).not.toBeNull();
});

it("ManagerView sorts active plans like the ContextTree", () => {
  const red = { state: "idle", attention: "red" } as const;
  const old = plan("old", { createdAt: "2026-01-01T00:00:00Z" });
  const fresh = plan("fresh", { createdAt: "2026-03-01T00:00:00Z" });
  const urgent = plan("urgent", { createdAt: "2025-01-01T00:00:00Z", sessions: [lead("lead-urgent", red)], leadSessionId: "lead-urgent" });
  const { container } = show(ctx([old, fresh, urgent]));
  const ids = qa(container, "li[data-manager-plan]").map((li) => li.getAttribute("data-manager-plan"));
  expect(ids).toEqual(["urgent", "fresh", "old"]);
});

it("ManagerView clicking a plan selects the plan and its lead session", () => {
  const p = plan("a");
  const orphan = plan("b", { leadSessionId: null, sessions: [] });
  const { container, onSelectPlan, onSelectSession } = show(ctx([p, orphan]));
  act(() => { q(row(container, "a"), '[role="button"]')!.click(); });
  expect(onSelectPlan).toHaveBeenCalledWith("a");
  expect(onSelectSession).toHaveBeenCalledWith("lead-a");
  expect(onSelectPlan.mock.invocationCallOrder[0]!).toBeLessThan(onSelectSession.mock.invocationCallOrder[0]!);
  act(() => { q(row(container, "b"), '[role="button"]')!.click(); });
  expect(onSelectPlan).toHaveBeenLastCalledWith("b");
  expect(onSelectSession).toHaveBeenCalledTimes(1);
});

it("ManagerView never scrolls horizontally", () => {
  const { container } = show(ctx([plan("a")]));
  const section = q(container, "[data-manager-view]")!;
  expect(section).not.toBeNull();
  expect(section.classList.contains("min-w-0")).toBe(true);
  expect(section.classList.contains("overflow-x-hidden")).toBe(true);
  const r = row(container, "a");
  expect(r.classList.contains("min-w-0")).toBe(true);
  expect(q(r, '[role="button"]')!.classList.contains("overflow-hidden") || r.classList.contains("overflow-x-hidden")).toBe(true);
});

it("ManagerView shows an empty note when the context has no plans", () => {
  const { container } = show(ctx([]));
  expect(q(container, "[data-empty]")!.textContent).toContain("No plans in this context yet");
});

it("ManagerView status dot matches the ContextTree Dot for the same plan", () => {
  const red = { state: "idle", attention: "red" } as const;
  const p = plan("a", { sessions: [lead("lead-a", red)], leadSessionId: "lead-a" });
  const { container } = show(ctx([p]));
  const actual = q(row(container, "a"), "[data-dot]")!;
  const ref = mount(<Dot dot={red} plan={p} archived={false} />);
  try {
    const expected = ref.container.querySelector("[data-dot]")!;
    expect(actual).not.toBeNull();
    for (const a of ["data-dot", "data-attention", "data-tone", "data-glyph", "aria-label", "class"]) {
      expect(actual.getAttribute(a)).toBe(expected.getAttribute(a));
    }
  } finally { ref.unmount(); }
});
