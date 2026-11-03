// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
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
  expect(m.container.querySelector('section[aria-label="Plan p1"] h2')?.textContent).toContain("Web UI");
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

it("shows an empty state when no plan is selected", () => {
  const m = mount(<PlanView plan={null} onAction={vi.fn()} />);
  expect(m.container.querySelector("p[data-empty]")?.textContent).toBe("No plan selected");
  expect(m.container.querySelector("section")).toBeNull();
  m.unmount();
});
