// @vitest-environment jsdom
import { act } from "react";
import { expect, it } from "vitest";
import type { Dialog, DialogResponse, Question } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { AskCard } from "./AskCard.tsx";
import { PlanReviewCard } from "./PlanReviewCard.tsx";

type AskDialog = Extract<Dialog, { method: "ask_user_question" }>;
type PlanDialog = Extract<Dialog, { method: "cpd_plan_review" }>;

function q(n: number, extra: Partial<Question> = {}): Question {
  return {
    header: `H${n}`,
    question: `Question ${n}?`,
    options: [
      { label: `A${n}`, description: `desc A${n}` },
      { label: `B${n}`, description: `desc B${n}` },
      { label: `C${n}`, description: `desc C${n}` },
      { label: `D${n}`, description: `desc D${n}` },
    ],
    ...extra,
  };
}

function ask(questions: Question[], recommended?: { questionIndex: number; label: string }[]) {
  const dialog: AskDialog = { id: "d1", method: "ask_user_question", interactive: true, questions };
  const responses: DialogResponse[] = [];
  const view = mount(
    <AskCard
      dialog={dialog}
      readOnly={false}
      onRespond={(r) => responses.push(r)}
      {...(recommended ? { recommended } : {})}
    />,
  );
  return { ...view, responses };
}

function plan() {
  const dialog: PlanDialog = {
    id: "review:r1",
    method: "cpd_plan_review",
    interactive: true,
    title: "Ship the widget",
    summary: "Three **bold** steps",
  };
  const responses: DialogResponse[] = [];
  const view = mount(
    <PlanReviewCard
      dialog={dialog}
      readOnly={false}
      onRespond={(r) => responses.push(r)}
      review={{
        revision: "r2",
        previous: { revision: "r1", verdict: "changes requested" },
        workflows: [
          { name: "write-path", route: "cpd → main", status: "draft", summary: "2 jobs", jobs: [
            { name: "a", role: "worker", tier: "S1" },
            { name: "b", role: "worker", tier: "S1" },
          ] },
        ],
        risks: ["slow"],
      }}
    />,
  );
  return { ...view, responses };
}

function byText<T extends HTMLElement>(c: HTMLElement, sel: string, text: string): T {
  const el = Array.from(c.querySelectorAll<T>(sel)).find((e) => (e.textContent ?? "").includes(text));
  if (!el) throw new Error(`no ${sel} with ${text}`);
  return el;
}

function click(el: HTMLElement) {
  act(() => el.click());
}

it("ask card renders question tabs and numbered options", () => {
  const { container, unmount } = ask([q(0), q(1)]);
  const tabs = container.querySelectorAll('[role="tablist"] [role="tab"][data-part="tab"]');
  expect(tabs.length).toBe(2);
  const badges = Array.from(
    container.querySelectorAll('fieldset[data-question="0"] [data-part="option"] [data-part="badge"]'),
  ).map((b) => b.textContent);
  expect(badges).toEqual(["1", "2", "3", "4"]);
  unmount();
});

it("ask card shows the recommended badge", () => {
  const { container, unmount } = ask([q(0)], [{ questionIndex: 0, label: "B0" }]);
  const badges = container.querySelectorAll('[data-part="recommended"]');
  expect(badges.length).toBe(1);
  expect(badges[0]?.textContent).toBe("recommended");
  expect(badges[0]?.closest('[data-option="B0"]')).not.toBeNull();
  unmount();
});

it("ask card shows the preview pane only when an option has a preview", () => {
  const plain = ask([q(0)]);
  expect(plain.container.querySelector('[data-layout="single"]')).not.toBeNull();
  expect(plain.container.querySelector('[data-layout="side-by-side"]')).toBeNull();
  expect(plain.container.querySelector('[data-part="preview-pane"]')).toBeNull();
  plain.unmount();

  const base = q(1);
  const withPreview = ask([
    { ...base, options: base.options.map((o, i) => (i === 0 ? { ...o, preview: "**alpha preview**" } : o)) },
  ]);
  const c = withPreview.container;
  expect(c.querySelector('[data-layout="side-by-side"]')).not.toBeNull();
  expect(c.querySelector('[data-part="preview-pane"]')).not.toBeNull();
  expect(c.querySelector('[data-part="preview"]')?.textContent).toBe("");
  click(c.querySelector<HTMLElement>('input[value="A1"]') as HTMLElement);
  expect(c.querySelector('[data-part="preview"] strong')?.textContent).toBe("alpha preview");
  expect(c.querySelector('[data-part="preview-focus"]')?.textContent).toBe("1 · A1");
  withPreview.unmount();
});

it("ask card renders the dashed type something input", () => {
  const { container, unmount } = ask([q(0)]);
  const custom = container.querySelector('[data-part="custom"]');
  expect(custom).not.toBeNull();
  const input = custom?.querySelector<HTMLInputElement>('input[type="text"]');
  expect(input?.placeholder).toBe("Type something.");
  expect(custom?.querySelector('[data-part="badge"]')?.textContent).toBe("5");
  unmount();
});

it("ask card submit footer counts answered questions", () => {
  const { container, unmount } = ask([q(0), q(1)]);
  const submit = () => container.querySelector('button[type="submit"]')?.textContent ?? "";
  expect(submit()).toContain("Submit 0/2 answers");
  expect(submit()).toContain("⏎");
  click(container.querySelector<HTMLElement>('input[value="A0"]') as HTMLElement);
  expect(submit()).toContain("Submit 1/2 answers");
  click(container.querySelector<HTMLElement>('input[value="B1"]') as HTMLElement);
  expect(submit()).toContain("Submit 2/2 answers");
  expect(container.querySelector('[data-part="hints"]')?.textContent).toBe(
    "↑↓ move · 1–4 pick · ⇥ next question · ⏎ submit · esc cancel",
  );
  unmount();
});

it("plan review card renders the revision header and section nav", () => {
  const { container, unmount } = plan();
  const header = container.querySelector('[data-part="header"]');
  expect(header).not.toBeNull();
  expect(header?.textContent).toContain("Plan review ·");
  expect(header?.querySelector("h3")?.textContent).toBe("Ship the widget");
  expect(header?.querySelector('[data-part="previous"]')?.textContent).toContain("r1");
  const nav = container.querySelector('nav[aria-label="Plan sections"]');
  expect(nav).not.toBeNull();
  const sections = Array.from(nav?.querySelectorAll("a[data-section]") ?? []).map((a) =>
    a.getAttribute("data-section"),
  );
  expect(sections).toEqual(["outline", "workflows", "jobs", "risks"]);
  expect(nav?.querySelector('a[data-section="outline"]')?.getAttribute("href")).toBe("#review:r1-outline");
  unmount();
});

it("plan review card renders the decision bar with three actions", () => {
  const { container, unmount } = plan();
  const bar = container.querySelector('footer[data-part="decision-bar"]');
  expect(bar).not.toBeNull();
  expect(byText(container, "button", "Reject plan").getAttribute("data-action")).toBe("reject");
  expect(byText(container, "button", "Request changes").getAttribute("data-action")).toBe("request-changes");
  expect(byText(container, "button", "Approve & lock").textContent).toContain("⌘⏎");
  expect(container.querySelector("textarea")).toBeNull();
  click(byText(container, "button", "Request changes"));
  const area = container.querySelector("textarea");
  expect(area?.getAttribute("aria-label")).toBe("Feedback");
  expect(area?.getAttribute("placeholder")).toBe("Feedback for the Lead — required to request changes");
  unmount();
});

it("plan review card sends feedback with the decision", () => {
  const { container, responses, unmount } = plan();
  click(byText(container, "button", "Approve & lock"));
  click(byText(container, "button", "Reject plan"));
  click(byText(container, "button", "Request changes"));
  const area = container.querySelector("textarea") as HTMLTextAreaElement;
  const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
  act(() => {
    set?.call(area, "  tighten scope  ");
    area.dispatchEvent(new Event("input", { bubbles: true }));
  });
  click(byText(container, "button", "Send feedback"));
  expect(responses).toEqual([
    { method: "cpd_plan_review", outcome: "approve" },
    { method: "cpd_plan_review", outcome: "reject" },
    { method: "cpd_plan_review", outcome: "changes", feedback: "tighten scope" },
  ]);
  unmount();
});
