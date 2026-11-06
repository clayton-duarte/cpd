// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import type { PlanViewModel, WorkflowView } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { PlanView } from "./PlanView.tsx";

const url = "https://github.com/acme/api/pull/7";
type Pr = NonNullable<WorkflowView["pr"]>;
const wf = (pr: Pr | null, status = "open"): WorkflowView => ({
  key: "wf1", title: "WF", repo: "acme/api", base: "main", status, needs: [], jobKeys: [], pr,
});
const pr = (over: Record<string, unknown> = {}): Pr =>
  ({ number: 7, url, state: "open", checks: null, mergeable: null, ...over }) as Pr;
const plan = (w: WorkflowView[], extra: Partial<PlanViewModel> = {}): PlanViewModel => ({
  planId: "p1", contextId: "c1", title: "Plan", phase: "dispatch",
  counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 },
  review: null, openEscalations: 0, cursor: 1, workflows: w, jobs: [], ...extra,
});
const post = (c: ParentNode): Element | null => c.querySelector("[data-action=post-pr]");

it("WorkflowCard shows Post PR only for an unposted open PR that is postable", () => {
  const show = (p: Pr, set?: ReadonlySet<string>) => {
    const m = mount(<PlanView plan={plan([wf(p)])} onAction={vi.fn()} {...(set === undefined ? {} : { postableWorkflows: set })} />);
    const found = post(m.container) !== null;
    m.unmount();
    return found;
  };
  expect(show(pr({ posted: false }))).toBe(true);
  expect(show(pr({ posted: false }), new Set(["wf1"]))).toBe(true);
  expect(show(pr({ posted: false }), new Set([url]))).toBe(true);
  expect(show(pr({ posted: false }), new Set(["#7"]))).toBe(true);
  expect(show(pr({ posted: true }))).toBe(false);
});

it("WorkflowCard hides Post PR when the PR is merged", () => {
  const m = mount(<PlanView plan={plan([wf(pr({ state: "merged", posted: false }))])} onAction={vi.fn()} />);
  expect(post(m.container)).toBeNull();
  m.unmount();
  const s = mount(<PlanView plan={plan([wf(pr({ posted: false }), "merged")])} onAction={vi.fn()} />);
  expect(post(s.container)).toBeNull();
  s.unmount();
});

it("WorkflowCard hides Post PR when postableWorkflows does not list the workflow", () => {
  const m = mount(<PlanView plan={plan([wf(pr({ posted: false }))])} onAction={vi.fn()} postableWorkflows={new Set(["other"])} />);
  expect(post(m.container)).toBeNull();
  m.unmount();
});

function expectIcon(link: Element | null | undefined): void {
  expect(link).not.toBeNull();
  expect(link?.getAttribute("target")).toBe("_blank");
  const last = link?.lastElementChild;
  expect(last?.tagName.toLowerCase()).toBe("svg");
  expect(last?.getAttribute("aria-hidden")).toBe("true");
}

it("WorkflowCard appends an external-link icon to the PR link", () => {
  const m = mount(<PlanView plan={plan([wf(pr({ posted: false }))])} onAction={vi.fn()} />);
  expectIcon(m.container.querySelector("[data-part=pr] a"));
  m.unmount();
});

it("ReviewPrCard appends an external-link icon to the PR link", () => {
  const p = plan([], { reviewedPr: { repo: "acme/api", number: 7, url, title: "T", state: "open" } });
  const m = mount(<PlanView plan={p} onAction={vi.fn()} />);
  expectIcon(m.container.querySelector("[data-part=pr] a"));
  expect(m.container.querySelector("[data-part=pr]")?.textContent).not.toContain("󱆭");
  m.unmount();
});

it("WorkflowCard DAG body scrolls horizontally inside a w-fit container", () => {
  const job = { key: "j1", title: "J", kind: "work", tier: "S1", status: "queued", needs: [], attempts: [], workflow: "wf1" };
  const w = { ...wf(null), jobKeys: ["j1"] };
  const m = mount(<PlanView plan={plan([w], { jobs: [job] as never })} onAction={vi.fn()} />);
  const body = m.container.querySelector("[data-part=dag-body]");
  expect(body).not.toBeNull();
  expect(body?.querySelector("[data-slot=scroll-area-content] div.w-fit")).not.toBeNull();
  m.unmount();
});
