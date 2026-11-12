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

  it("routes inter-workflow edges in left gutter with distinct lanes avoiding workflow cards", () => {
    // 3 stacked workflows: wf-1 -> wf-2, wf-2 -> wf-3, wf-1 -> wf-3.
    // wf-1 feeds wf-3 with wf-2 between them, plus parallel edges passing/touching intermediate card.
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
      {
        key: "wf-3",
        title: "Workflow 3",
        repo: "repo",
        base: "main",
        needs: ["wf-1", "wf-2"],
        status: "open",
        jobKeys: [],
        pr: null,
      },
    ]);

    const m = mount(<PlanView plan={plan} onAction={vi.fn()} />);

    const canvas = m.container.querySelector('[data-part="wf-canvas"]') as HTMLElement;
    expect(canvas).not.toBeNull();

    const edge1to3 = m.container.querySelector('path[data-wf-edge="wf-1>wf-3"]');
    const edge1to2 = m.container.querySelector('path[data-wf-edge="wf-1>wf-2"]');
    const edge2to3 = m.container.querySelector('path[data-wf-edge="wf-2>wf-3"]');

    expect(edge1to3).not.toBeNull();
    expect(edge1to2).not.toBeNull();
    expect(edge2to3).not.toBeNull();

    // 1) Every inter-Workflow edge path segment has x < workflowCardLeft
    // (no segment intersects any Workflow card's bounding box).
    // The edge path is M{xL} {ys}H{lx}V{yt}H{xL} where lx < workflowCardLeft,
    // and stubs touch the card only at its left edge (x = xL = workflowCardLeft).
    // Specifically, for wf-1 -> wf-3 passing intermediate card wf-2, the vertical run and path
    // must run in the left gutter strictly to the left of the cards, never crossing intermediate cards.
    const d1to3 = edge1to3?.getAttribute("d") ?? "";
    const d1to2 = edge1to2?.getAttribute("d") ?? "";
    const d2to3 = edge2to3?.getAttribute("d") ?? "";

    // The legacy curve M...C... cut through the middle of intermediate cards.
    // Under left-gutter routing, paths use orthogonal segments M...H...V...H... avoiding cards.
    expect(d1to3).not.toContain("C");
    expect(d1to2).not.toContain("C");
    expect(d2to3).not.toContain("C");

    // Path segments syntax: M{xL} {ys}H{lx}V{yt}H{xL}
    const pathRegex = /^M\s*(\d+(?:\.\d+)?)\s+(\d+(?:\.\d+)?)\s*H\s*(\d+(?:\.\d+)?)\s*V\s*(\d+(?:\.\d+)?)\s*H\s*(\d+(?:\.\d+)?)$/;
    const m1to3 = d1to3.match(pathRegex);
    expect(m1to3).not.toBeNull();
    if (m1to3) {
      const [, startX, , laneX, , endX] = m1to3.map(Number);
      // Leaves and enters at the card left edge
      expect(startX).toBe(endX);
      // The vertical segment is placed in the left gutter outside the cards (x < workflowCardLeft)
      expect(laneX).toBeLessThan(startX);
    }

    // 2) Parallel edges passing an intermediate card get distinct x tracks/lanes in the left gutter
    // so parallel edges do not overlap each other or touch cards.
    const lane1to3 = edge1to3?.getAttribute("data-lane");
    const lane1to2 = edge1to2?.getAttribute("data-lane");
    const lane2to3 = edge2to3?.getAttribute("data-lane");

    expect(lane1to3).not.toBeNull();
    expect(lane1to2).not.toBeNull();
    expect(lane2to3).not.toBeNull();
    // Conflicting/parallel edges across intermediate cards must have distinct lanes
    const lanes = new Set([lane1to3, lane1to2, lane2to3]);
    expect(lanes.size).toBe(3);

    // 3) Arrow leaves source Workflow on left edge and enters target Workflow on left edge with an arrowhead
    const arrowheads = Array.from(m.container.querySelectorAll('[data-part="wf-arrowhead"]'));
    expect(arrowheads.length).toBe(3);

    // Arrowhead pointing right into target's left edge: d = M{xL-6} {yt-4}L{xL} {yt}L{xL-6} {yt+4}
    // Must not be the legacy downward-pointing arrowhead M{x2-4} {y2-6}L{x2} {y2}L{x2+4} {y2-6}
    const arrowD1to3 = arrowheads[0]?.getAttribute("d") ?? "";
    const arrowRegex = /^M\s*(\d+(?:\.\d+)?)\s+(\d+(?:\.\d+)?)\s*L\s*(\d+(?:\.\d+)?)\s+(\d+(?:\.\d+)?)\s*L\s*(\d+(?:\.\d+)?)\s+(\d+(?:\.\d+)?)$/;
    expect(arrowD1to3).toMatch(arrowRegex);
    const arrowMatch = arrowD1to3.match(arrowRegex);
    if (arrowMatch) {
      const [, ax1, ay1, tipX, tipY, ax2, ay2] = arrowMatch.map(Number);
      // Tip points to the card's left boundary: tipX > ax1 and tipX > ax2 (pointing right)
      expect(tipX).toBeGreaterThan(ax1);
      expect(tipX).toBeGreaterThan(ax2);
      expect(ay1).toBeLessThan(tipY);
      expect(ay2).toBeGreaterThan(tipY);
    }

    m.unmount();
  });
});

