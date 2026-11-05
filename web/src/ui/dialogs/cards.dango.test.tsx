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

const choice = (c: HTMLElement, label: string) => c.querySelector<HTMLElement>(`[data-slot="questionnaire-choice"][data-option="${label}"]`) as HTMLElement;
const step = (c: HTMLElement, i: number) => c.querySelector<HTMLElement>(`[data-slot="questionnaire-item"][data-question="${i}"]`) as HTMLElement;
const next = (c: HTMLElement) => c.querySelector<HTMLElement>('[data-slot="questionnaire-next"]') as HTMLElement;

it("ask card renders a questionnaire with one lettered step per question", () => {
  const { container, unmount } = ask([q(0), q(1)]);
  expect(container.querySelector('[data-slot="questionnaire"][data-dialog="ask_user_question"]')).not.toBeNull();
  expect(container.querySelector('[data-slot="questionnaire-progress"]')).not.toBeNull();
  expect(container.querySelectorAll('[data-slot="questionnaire-item"]').length).toBe(2);
  expect(step(container, 0).hasAttribute("data-active")).toBe(true);
  const options = Array.from(step(container, 0).querySelectorAll('[data-part="options"] [data-part="option"]')).map((o) => o.getAttribute("data-option"));
  expect(options).toEqual(["A0", "B0", "C0", "D0"]);
  const letters = Array.from(step(container, 0).querySelectorAll('[data-slot="questionnaire-choice-shortcut"]')).map((b) => b.textContent);
  expect(letters).toEqual(["A", "B", "C", "D"]);
  expect(container.querySelector('[data-slot="questionnaire"]')?.getAttribute("data-shortcuts")).toBe("letters");
  unmount();
});

it("ask card shows the recommended mark", () => {
  const { container, unmount } = ask([q(0)], [{ questionIndex: 0, label: "B0" }]);
  expect(choice(container, "B0").textContent).toContain("B0 (Recommended)");
  expect(choice(container, "A0").textContent).not.toContain("Recommended");
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
  click(choice(c, "A1"));
  expect(c.querySelector('[data-part="preview"] strong')?.textContent).toBe("alpha preview");
  expect(c.querySelector('[data-part="preview-focus"]')?.textContent).toBe("A1");
  withPreview.unmount();
});

it("ask card renders the type something input on every step", () => {
  const { container, unmount } = ask([q(0), q(1)]);
  for (const i of [0, 1]) {
    const input = step(container, i).querySelector<HTMLInputElement>('[data-slot="questionnaire-input"]');
    expect(input?.getAttribute("aria-label")).toBe("Type something.");
    expect(input?.placeholder).toBe("Type something.");
  }
  unmount();
});

it("ask card submits from the last step with the answers of every step", () => {
  const { container, responses, unmount } = ask([q(0), q(1)]);
  expect(container.querySelector('[data-slot="questionnaire-submit"]')?.hasAttribute("hidden"), "Submit belongs to the last step").toBe(true);
  click(choice(container, "A0"));
  click(next(container));
  click(choice(container, "B1"));
  click(container.querySelector<HTMLElement>('[data-slot="questionnaire-submit"]') as HTMLElement);
  expect(responses).toEqual([
    {
      method: "ask_user_question",
      outcome: "answered",
      result: {
        answers: [
          { questionIndex: 0, question: "Question 0?", kind: "option", answer: "A0" },
          { questionIndex: 1, question: "Question 1?", kind: "option", answer: "B1" },
        ],
        cancelled: false,
      },
    },
  ]);
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
  const bar = container.querySelector('[data-slot="item-footer"][data-part="decision-bar"]');
  expect(bar).not.toBeNull();
  const actions = container.querySelector('[data-slot="item-actions"][data-part="actions"]') as HTMLElement;
  expect(byText(actions, "button", "Reject plan").getAttribute("data-action")).toBe("reject");
  expect(byText(actions, "button", "Request changes").getAttribute("data-action")).toBe("request-changes");
  expect(byText(actions, "button", "Approve & lock").textContent).toContain("⌘⏎");
  expect(container.querySelector("textarea")).toBeNull();
  click(byText(container, "button", "Request changes"));
  const area = container.querySelector("textarea");
  expect(area?.getAttribute("aria-label")).toBe("Feedback");
  expect(byText(container, "button", "Request changes").getAttribute("aria-expanded")).toBe("true");
  unmount();
});

it("plan review card sends each decision once and then disables the actions", () => {
  const approve = plan();
  click(byText(approve.container, "button", "Approve & lock"));
  expect(approve.responses).toEqual([{ method: "cpd_plan_review", outcome: "approve" }]);
  click(byText(approve.container, "button", "Reject plan"));
  expect(approve.responses.length, "the buttons are disabled while submitting").toBe(1);
  approve.unmount();
  const reject = plan();
  click(byText(reject.container, "button", "Reject plan"));
  expect(reject.responses).toEqual([{ method: "cpd_plan_review", outcome: "reject" }]);
  reject.unmount();
});

it("plan review card sends feedback with the decision", () => {
  const { container, responses, unmount } = plan();
  click(byText(container, "button", "Request changes"));
  const area = container.querySelector("textarea") as HTMLTextAreaElement;
  const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
  act(() => {
    set?.call(area, "  tighten scope  ");
    area.dispatchEvent(new Event("input", { bubbles: true }));
  });
  click(byText(container, "button", "Send feedback"));
  expect(responses).toEqual([{ method: "cpd_plan_review", outcome: "changes", feedback: "tighten scope" }]);
  unmount();
});
