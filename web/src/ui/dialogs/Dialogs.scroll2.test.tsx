// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Dialog, Question } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { AskCard } from "./AskCard.tsx";
import { LegendDialog } from "./LegendDialog.tsx";
import { PlanReviewCard } from "./PlanReviewCard.tsx";

// Radix and Base UI (external) observe sizes in jsdom: inert double.
beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
  Element.prototype.scrollIntoView = vi.fn();
  Element.prototype.hasPointerCapture = vi.fn(() => false);
});
afterEach(() => vi.unstubAllGlobals());

const SLOT = '[data-slot="scroll-area"]';

it("dialog content scrolls through a ScrollArea", () => {
  const { unmount } = mount(<LegendDialog open onClose={vi.fn()} />);
  const content = document.body.querySelector('[data-slot="dialog-content"]');
  expect(content).not.toBeNull();
  expect(content?.classList.contains("overflow-hidden")).toBe(true);
  expect(content?.classList.contains("overflow-y-auto")).toBe(false);
  const area = content?.querySelector(SLOT);
  expect(area).not.toBeNull();
  const title = content?.querySelector("h2");
  expect(title).not.toBeNull();
  expect(area?.contains(title ?? null)).toBe(true);
  unmount();
});

it("ask card scrolls through a ScrollArea", () => {
  const questions: Question[] = [
    {
      header: "H0",
      question: "Question 0?",
      options: [
        { label: "A0", description: "d", preview: "alpha preview" },
        { label: "B0", description: "d", preview: "beta preview" },
      ],
    },
  ];
  const dialog: Extract<Dialog, { method: "ask_user_question" }> = {
    id: "d1", method: "ask_user_question", interactive: true, questions,
  };
  const { container, unmount } = mount(<AskCard dialog={dialog} readOnly={false} onRespond={vi.fn()} />);
  const form = container.querySelector('form[data-dialog="ask_user_question"]');
  const card = form?.parentElement ?? null;
  expect(card).not.toBeNull();
  expect(card?.matches(SLOT) === true || card?.querySelector(SLOT) !== null).toBe(true);
  expect(container.querySelectorAll(".overflow-y-auto, .overflow-auto").length).toBe(0);
  const preview = container.querySelector('[data-part="preview"]');
  expect(preview?.closest(SLOT)).not.toBeNull();
  unmount();
});

it("plan review card body scrolls through a ScrollArea", () => {
  const dialog: Extract<Dialog, { method: "cpd_plan_review" }> = {
    id: "review:r1", method: "cpd_plan_review", interactive: true, title: "Ship the widget", summary: "Three steps",
  };
  const { container, unmount } = mount(<PlanReviewCard dialog={dialog} readOnly={false} onRespond={vi.fn()} />);
  const card = container.querySelector('section[data-dialog="cpd_plan_review"]');
  const body = container.querySelector('[data-part="body"]');
  expect(body).not.toBeNull();
  const area = body?.closest(SLOT) ?? null;
  expect(area).not.toBeNull();
  expect(card?.contains(area) === true).toBe(true);
  expect(body?.classList.contains("overflow-y-auto")).toBe(false);
  unmount();
});
