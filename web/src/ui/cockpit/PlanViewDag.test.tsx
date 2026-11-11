// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import type { PlanViewModel, WorkflowView } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { PlanView } from "./PlanView.tsx";

function createPlan(workflows: WorkflowView[]): PlanViewModel {
  return {
    planId: "p1",
    contextId: "c1",
    title: "DAG Plan",
    phase: "dispatch",
    counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 },
    review: null,
    openEscalations: 0,
    cursor: 1,
    workflows,
    jobs: [],
  };
}

describe("PlanView DAG", () => {
  it("renders base edge between stacked workflow cards", () => {
    const plan = createPlan([
      {
        key: "wf-base",
        title: "Base Workflow",
        repo: "repo",
        base: "main",
        status: "open",
        needs: [],
        jobKeys: [],
        pr: null,
      },
      {
        key: "wf-child",
        title: "Child Workflow",
        repo: "repo",
        base: "cpd/p1/wf-base",
        base_workflow: "p1/wf-base",
        status: "open",
        needs: [],
        jobKeys: [],
        pr: null,
      },
    ]);

    const m = mount(<PlanView plan={plan} onAction={vi.fn()} />);
    const edge = m.container.querySelector('path[data-wf-edge="wf-base>wf-child"]');
    expect(edge).not.toBeNull();
    expect(edge?.getAttribute("data-edge-kind")).toBe("base");
    expect(edge?.getAttribute("stroke-dasharray")).toBeNull();
    m.unmount();
  });

  it("renders needs edge between dependent workflow cards", () => {
    const plan = createPlan([
      {
        key: "wf-a",
        title: "Workflow A",
        repo: "repo",
        base: "main",
        status: "open",
        needs: [],
        jobKeys: [],
        pr: null,
      },
      {
        key: "wf-b",
        title: "Workflow B",
        repo: "repo",
        base: "main",
        status: "open",
        needs: ["wf-a"],
        jobKeys: [],
        pr: null,
      },
    ]);

    const m = mount(<PlanView plan={plan} onAction={vi.fn()} />);
    const edge = m.container.querySelector('path[data-wf-edge="wf-a>wf-b"]');
    expect(edge).not.toBeNull();
    expect(edge?.getAttribute("data-edge-kind")).toBe("needs");
    expect(edge?.getAttribute("stroke-dasharray")).toBe("3 3");
    m.unmount();
  });

  it("orders workflow cards topologically", () => {
    // Input order is child first, parent second.
    // Topological sort should order upstream (wf-parent) before dependant (wf-child).
    const plan = createPlan([
      {
        key: "wf-child",
        title: "Child Workflow",
        repo: "repo",
        base: "main",
        needs: ["wf-parent"],
        status: "open",
        jobKeys: [],
        pr: null,
      },
      {
        key: "wf-parent",
        title: "Parent Workflow",
        repo: "repo",
        base: "main",
        needs: [],
        status: "open",
        jobKeys: [],
        pr: null,
      },
    ]);

    const m = mount(<PlanView plan={plan} onAction={vi.fn()} />);
    const cards = Array.from(m.container.querySelectorAll("section[data-workflow]")).map((el) =>
      el.getAttribute("data-workflow"),
    );
    expect(cards).toEqual(["wf-parent", "wf-child"]);
    m.unmount();
  });

  it("renders without workflow edges when plan has no workflow dependencies", () => {
    const plan = createPlan([
      {
        key: "wf-1",
        title: "Independent 1",
        repo: "repo",
        base: "main",
        status: "open",
        needs: [],
        jobKeys: [],
        pr: null,
      },
      {
        key: "wf-2",
        title: "Independent 2",
        repo: "repo",
        base: "main",
        status: "open",
        needs: [],
        jobKeys: [],
        pr: null,
      },
    ]);

    const m = mount(<PlanView plan={plan} onAction={vi.fn()} />);
    expect(m.container.querySelector('[data-part="wf-edges"]')).toBeNull();
    expect(m.container.querySelector('[data-part="wf-legend"]')).toBeNull();
    const cards = Array.from(m.container.querySelectorAll("section[data-workflow]")).map((el) =>
      el.getAttribute("data-workflow"),
    );
    // Preserves input order
    expect(cards).toEqual(["wf-1", "wf-2"]);
    // But wrapper wf-canvas still renders per DESIGN-panes 1e(iv) 5
    expect(m.container.querySelector('[data-part="wf-canvas"]')).not.toBeNull();
    m.unmount();
  });

  it("renders legend for workflow dependency edges", () => {
    const plan = createPlan([
      {
        key: "wf-1",
        title: "Workflow 1",
        repo: "repo",
        base: "main",
        status: "open",
        needs: [],
        jobKeys: [],
        pr: null,
      },
      {
        key: "wf-2",
        title: "Workflow 2",
        repo: "repo",
        base: "main",
        needs: ["wf-1"],
        status: "open",
        jobKeys: [],
        pr: null,
      },
    ]);

    const m = mount(<PlanView plan={plan} onAction={vi.fn()} />);
    const legend = m.container.querySelector('[data-part="wf-legend"]');
    expect(legend).not.toBeNull();
    expect(legend?.textContent).toContain("Stacked (base)");
    expect(legend?.textContent).toContain("Depends on (needs)");
    m.unmount();
  });
});
