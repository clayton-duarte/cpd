// @vitest-environment jsdom
// Red tests for the consolidated cards (DESIGN-compose 4, 5; pro-d56): AskCard is a multi-step
// Questionnaire; PlanReviewCard is a Confirmation Item.
import { act } from "react";
import { expect, it } from "vitest";
import type { Dialog, DialogResponse, Question } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { AskCard } from "./AskCard.tsx";
import { PlanReviewCard } from "./PlanReviewCard.tsx";

const q = (n: number): Question => ({
  header: `H${n}`, question: `Question ${n}?`,
  options: [{ label: `A${n}`, description: `desc A${n}` }, { label: `B${n}`, description: `desc B${n}` }],
});

it("ask card steps through the questions as a questionnaire", () => {
  const responses: DialogResponse[] = [];
  const dialog: Extract<Dialog, { method: "ask_user_question" }> = { id: "d1", method: "ask_user_question", interactive: true, questions: [q(0), q(1)] };
  const m = mount(<AskCard dialog={dialog} readOnly={false} onRespond={(r) => responses.push(r)} />);
  const root = m.container.querySelector('[data-slot="questionnaire"][aria-label="Questions"][data-dialog="ask_user_question"]');
  expect(root, "the card is a Questionnaire").not.toBeNull();
  expect(root?.querySelector('[data-slot="questionnaire-progress"]')).not.toBeNull();
  const step = (i: number) => root!.querySelector<HTMLElement>(`[data-slot="questionnaire-item"][data-question="${i}"]`)!;
  expect(step(0).hasAttribute("data-active"), "the first question is the current step").toBe(true);
  expect(step(1).hasAttribute("data-active")).toBe(false);
  expect(step(0).querySelector('[data-slot="questionnaire-title"]')?.textContent).toBe("H0: Question 0?");
  expect(step(0).querySelector('[data-option="A0"]')?.textContent).toContain("desc A0");
  expect(root!.querySelector('[data-slot="questionnaire-input"][aria-label="Type something."]')).not.toBeNull();
  act(() => step(0).querySelector<HTMLElement>('[data-option="A0"]')!.click());
  act(() => root!.querySelector<HTMLElement>('[data-slot="questionnaire-next"]')!.click());
  expect(step(1).hasAttribute("data-active"), "Next moves to the second question").toBe(true);
  expect(step(0).hasAttribute("data-active")).toBe(false);
  act(() => root!.querySelector<HTMLElement>('[data-slot="questionnaire-previous"]')!.click());
  expect(step(0).hasAttribute("data-active"), "Previous goes back").toBe(true);
  act(() => m.container.querySelector<HTMLElement>('button[data-action="cancel"]')!.click());
  expect(responses).toEqual([{ method: "ask_user_question", outcome: "cancelled" }]);
  m.unmount();
});

it("plan review card renders a confirmation item with approve, reject and changes actions", () => {
  const responses: DialogResponse[] = [];
  const dialog: Extract<Dialog, { method: "cpd_plan_review" }> = { id: "review:r1", method: "cpd_plan_review", interactive: true, title: "Ship the widget", summary: "Three steps" };
  const m = mount(<PlanReviewCard dialog={dialog} readOnly={false} onRespond={(r) => responses.push(r)} />);
  const card = m.container.querySelector<HTMLElement>('section[aria-label="Plan review"][data-dialog="cpd_plan_review"]');
  expect(card?.getAttribute("data-slot"), "the section is the Item frame").toBe("item");
  expect(card?.getAttribute("data-variant")).toBe("outline");
  expect(card?.getAttribute("data-size")).toBe("sm");
  expect(card?.querySelector('[data-slot="item-media"][data-part="phi"]')).not.toBeNull();
  expect(card?.querySelector('[data-slot="item-content"] h3')?.textContent).toBe("Ship the widget");
  const actions = card?.querySelector('[data-slot="item-actions"]');
  for (const a of ["reject", "request-changes", "approve"]) {
    expect(actions?.querySelector(`button[data-action="${a}"]`), a).not.toBeNull();
  }
  act(() => actions!.querySelector<HTMLElement>('button[data-action="request-changes"]')!.click());
  expect(card?.querySelector('textarea[aria-label="Feedback"]')).not.toBeNull();
  act(() => card!.querySelector<HTMLElement>('button[data-action="approve"]')!.click());
  expect(responses).toEqual([{ method: "cpd_plan_review", outcome: "approve" }]);
  m.unmount();
});
