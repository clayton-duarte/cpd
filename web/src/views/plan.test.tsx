// @vitest-environment jsdom
// The Plan view's needs/base links (docs/research/web-client-api.md §6, as amended by
// web-links-docs; hs-n673). The view draws, for every Job, a link to every Job in its
// `needs` (`jobs[].needs`), and for every PR a link to every PR in its `needs`
// (`prs[].needs`) and a distinct base link labelled `on <base_pr>` (`prs[].base_pr`).
//
// RED: today's plan.tsx renders neither the needs links nor the base link, so both tests
// fail. The DOM contract the impl Job must satisfy (the exact selectors pinned here):
//
//   * every rendered Job carries `data-job="<key>"`; inside it, one
//     `<a data-need="<need>" href="#job-<need>">` per need, its text the need key;
//   * every rendered PR carries `data-pr="<key>"`; inside it, one
//     `<a data-need="<need>" href="#pr-<need>">` per need, and a distinct
//     `<a data-base="<base_pr>" href="#pr-<base_pr>">` whose text is `on <base_pr>`.
//
// Rendering is synchronous, so this uses react-dom/server into a jsdom node: no effects,
// no act, no external dependency.
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import { Plan } from "./plan";
import type { PlanShowResult } from "../api/readmodel";

// A small `plan show` fixture (§3, docs/commands.md §4.3): a Job `j1` that needs `a`, a PR
// `p1` that needs `b` and is based on `c`.
const plan: PlanShowResult = {
  ok: true,
  plan: {
    id: "plan-1",
    context: "ctx-1",
    title: "job-needs",
    jira: null,
    phase: "active",
    lead_session: null,
    created_at: "2026-11-01T09:00:00-04:00",
    locked_at: null,
    done_at: null,
    archived_at: null,
    outline: null,
  },
  prs: [{ key: "p1", repo: "cpd", status: "running", needs: ["b"], base_pr: "c" }],
  jobs: [{ key: "j1", title: "the first job", status: "running", needs: ["a"] }],
  escalations: null,
  review: null,
  counts: null,
  tamper: null,
  events_cursor: "evt-1",
};

/** Renders `Plan` to static markup and parses it into a jsdom node for querying. */
function render(): HTMLElement {
  const container = document.createElement("div");
  container.innerHTML = renderToStaticMarkup(<Plan plan={plan} />);
  return container;
}

test("the Plan view links a Job to each of its needs", () => {
  const container = render();

  const link = container.querySelector('a[data-need="a"]');
  expect(link, "the Job's link to its need a").not.toBeNull();
  expect(link?.getAttribute("href")).toBe("#job-a");
  expect(link?.textContent).toBe("a");

  const job = container.querySelector('[data-job="j1"]');
  expect(job, "the Job's data-job anchor").not.toBeNull();
  expect(job?.contains(link)).toBe(true);
});

test("the Plan view links a PR to its needs and its base PR", () => {
  const container = render();

  const need = container.querySelector('a[data-need="b"]');
  expect(need, "the PR's link to its need b").not.toBeNull();
  expect(need?.getAttribute("href")).toBe("#pr-b");

  const base = container.querySelector('a[data-base="c"]');
  expect(base, "the PR's distinct base link").not.toBeNull();
  expect(base?.getAttribute("href")).toBe("#pr-c");
  expect(base?.textContent).toBe("on c");

  const pr = container.querySelector('[data-pr="p1"]');
  expect(pr, "the PR's data-pr anchor").not.toBeNull();
  expect(pr?.contains(need)).toBe(true);
  expect(pr?.contains(base)).toBe(true);
});
