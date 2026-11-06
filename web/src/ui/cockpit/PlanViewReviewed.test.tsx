// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import type { PlanViewModel } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { PlanView } from "./PlanView.tsx";

const url = "https://github.com/acme/api/pull/3";
const basePlan: PlanViewModel = {
  planId: "p1",
  contextId: "codereview",
  title: "Review acme/api#3",
  phase: "dispatch",
  counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 },
  review: null,
  openEscalations: 0,
  cursor: 1,
  workflows: [],
  jobs: [],
};
const withPr = (state: string | null): PlanViewModel => ({
  ...basePlan,
  reviewedPr: { repo: "acme/api", number: 3, url, title: "Fix the thing", state },
});

it("review plan shows a Review PR card with a link to the PR", () => {
  const m = mount(<PlanView plan={withPr("open")} onAction={vi.fn()} />);
  const link = m.container.querySelector(`a[href="${url}"]`);
  expect(link).not.toBeNull();
  expect(link?.textContent).toContain("#3 open");
  const card = link?.closest("section");
  expect(card?.textContent).toContain("Review PR");
  expect(card?.textContent).toContain("acme/api");
  expect(card?.textContent).toContain("Fix the thing");
  expect(m.container.querySelector('[data-part="title-row"]')?.contains(card ?? null)).toBe(false);
  m.unmount();
});

it("review plan card omits the state when unknown", () => {
  const m = mount(<PlanView plan={withPr(null)} onAction={vi.fn()} />);
  const link = m.container.querySelector(`a[href="${url}"]`);
  expect(link).not.toBeNull();
  expect(link?.textContent?.trim()).toBe("#3");
  expect(link?.textContent).not.toContain("null");
  m.unmount();
});

it("review plan without workflows does not say no workflows yet", () => {
  const m = mount(<PlanView plan={withPr("open")} onAction={vi.fn()} />);
  expect(m.container.textContent).not.toContain("No workflows yet");
  expect(m.container.querySelector(`a[href="${url}"]`)).not.toBeNull();
  m.unmount();
});

it("plan without reviewed PR and workflows still says no workflows yet", () => {
  const m = mount(<PlanView plan={{ ...basePlan, reviewedPr: null }} onAction={vi.fn()} />);
  expect(m.container.textContent).toContain("No workflows yet");
  expect(m.container.textContent).not.toContain("Review PR");
  m.unmount();
  // contrast: the same plan with a reviewed PR drops the empty text
  const r = mount(<PlanView plan={withPr("open")} onAction={vi.fn()} />);
  expect(r.container.textContent).not.toContain("No workflows yet");
  r.unmount();
});
