// @vitest-environment jsdom
import { act } from "react";
import { expect, it } from "vitest";
import type { Dialog, DialogResponse, Question } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { AskCard } from "./AskCard.tsx";

type AskDialog = Extract<Dialog, { method: "ask_user_question" }>;

function q(n: number, extra: Partial<Question> = {}): Question {
  return {
    header: `H${n}`,
    question: `Question ${n}?`,
    options: [
      { label: `A${n}`, description: `desc A${n}` },
      { label: `B${n}`, description: `desc B${n}` },
    ],
    ...extra,
  };
}

function dialog(questions: Question[], interactive = true): AskDialog {
  return { id: "d1", method: "ask_user_question", interactive, questions };
}

function setup(questions: Question[], readOnly = false) {
  const responses: DialogResponse[] = [];
  const view = mount(
    <AskCard dialog={dialog(questions, !readOnly)} readOnly={readOnly} onRespond={(r) => responses.push(r)} />,
  );
  return { ...view, responses };
}

function field(c: HTMLElement, i: number): HTMLElement {
  const el = c.querySelector<HTMLElement>(`fieldset[data-question="${i}"]`);
  if (!el) throw new Error(`no fieldset ${i}`);
  return el;
}

function input(c: HTMLElement, i: number, value: string): HTMLInputElement {
  const el = field(c, i).querySelector<HTMLInputElement>(`input[value="${value}"]`);
  if (!el) throw new Error(`no input ${value}`);
  return el;
}

function click(el: HTMLElement) {
  act(() => el.click());
}

function type(el: HTMLInputElement, text: string) {
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  act(() => {
    set?.call(el, text);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function submit(c: HTMLElement) {
  const b = c.querySelector<HTMLButtonElement>('button[type="submit"]');
  if (!b) throw new Error("no submit");
  click(b);
}

  it("renders 1 to 4 questions with options", () => {
    for (const n of [1, 4]) {
      const { container, unmount } = setup(Array.from({ length: n }, (_, i) => q(i)));
      expect(container.querySelector('form[aria-label="Questions"][data-dialog="ask_user_question"]')).not.toBeNull();
      expect(container.querySelectorAll("fieldset[data-question]").length).toBe(n);
      expect(field(container, 0).querySelector("legend")?.textContent).toContain("H0: Question 0?");
      expect(field(container, 0).textContent).toContain("desc A0");
      expect(input(container, 0, "A0").type).toBe("radio");
      expect(input(container, 0, "B0").type).toBe("radio");
      unmount();
    }
  });

  it("single-select picks one option", () => {
    const { container, unmount } = setup([q(0)]);
    click(input(container, 0, "A0"));
    click(input(container, 0, "B0"));
    expect(input(container, 0, "A0").checked).toBe(false);
    expect(input(container, 0, "B0").checked).toBe(true);
    unmount();
  });

  it("multi-select picks many options", () => {
    const { container, unmount } = setup([q(0, { multiSelect: true })]);
    expect(input(container, 0, "A0").type).toBe("checkbox");
    click(input(container, 0, "A0"));
    click(input(container, 0, "B0"));
    expect(input(container, 0, "A0").checked).toBe(true);
    expect(input(container, 0, "B0").checked).toBe(true);
    unmount();
  });

  it("the focused option preview renders side by side", () => {
    const question = q(0, {
      options: [
        { label: "A0", description: "d", preview: "**alpha preview**" },
        { label: "B0", description: "d", preview: "beta preview" },
      ],
    });
    const { container, unmount } = setup([question]);
    const layout = field(container, 0).querySelector('[data-layout="side-by-side"]');
    expect(layout).not.toBeNull();
    const pane = layout?.querySelector('[data-part="preview"]');
    expect(layout?.querySelector('[data-part="options"]')).not.toBeNull();
    expect(pane?.textContent).toBe("");
    click(input(container, 0, "A0"));
    expect(pane?.textContent).toContain("alpha preview");
    expect(pane?.querySelector("strong")?.textContent).toBe("alpha preview");
    click(input(container, 0, "B0"));
    expect(pane?.textContent).toContain("beta preview");
    expect(pane?.textContent).not.toContain("alpha");
    unmount();
  });

  it("a custom text row is appended automatically and submits typed text", () => {
    const { container, responses, unmount } = setup([q(0)]);
    click(input(container, 0, "A0"));
    const text = field(container, 0).querySelector<HTMLInputElement>('input[type="text"][aria-label="Type something."]');
    expect(text).not.toBeNull();
    type(text as HTMLInputElement, "my own answer");
    expect(input(container, 0, "A0").checked).toBe(false);
    submit(container);
    expect(responses).toEqual([
      {
        method: "ask_user_question",
        outcome: "answered",
        result: {
          answers: [{ questionIndex: 0, question: "Question 0?", kind: "custom", answer: "my own answer" }],
          cancelled: false,
        },
      },
    ]);
    unmount();
  });

  it("Escape cancels", () => {
    const { container, responses, unmount } = setup([q(0)]);
    const form = container.querySelector("form") as HTMLFormElement;
    act(() => {
      form.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(responses).toEqual([{ method: "ask_user_question", outcome: "cancelled" }]);
    click(container.querySelector('button[data-action="cancel"]') as HTMLElement);
    expect(responses.length).toBe(2);
    expect(responses[1]).toEqual({ method: "ask_user_question", outcome: "cancelled" });
    unmount();
  });

  it("submit sends answers per question index", () => {
    const questions = [
      q(0, { options: [{ label: "A0", description: "d", preview: "pv" }, { label: "B0", description: "d" }] }),
      q(1, {
        multiSelect: true,
        options: [
          { label: "A1", description: "d" },
          { label: "B1", description: "d" },
          { label: "C1", description: "d" },
        ],
      }),
      q(2),
    ];
    const { container, responses, unmount } = setup(questions);
    click(input(container, 0, "A0"));
    click(input(container, 1, "C1"));
    click(input(container, 1, "A1"));
    submit(container);
    expect(responses).toEqual([
      {
        method: "ask_user_question",
        outcome: "answered",
        result: {
          answers: [
            { questionIndex: 0, question: "Question 0?", kind: "option", answer: "A0", preview: "pv" },
            { questionIndex: 1, question: "Question 1?", kind: "multi", selected: ["A1", "C1"], answer: "" },
          ],
          cancelled: false,
          unanswered: [2],
        },
      },
    ]);
    unmount();
  });

  it("read-only mode renders the questions without controls", () => {
    const { container, responses, unmount } = setup([q(0), q(1)], true);
    const section = container.querySelector('section[aria-label="Questions"][data-dialog="ask_user_question"][data-readonly]');
    expect(section).not.toBeNull();
    expect(section?.textContent).toContain("H0: Question 0?");
    expect(section?.textContent).toContain("desc B1");
    expect(section?.querySelectorAll("ol > li").length).toBeGreaterThanOrEqual(2);
    expect(container.querySelector("input, button, textarea, form")).toBeNull();
    expect(container.textContent).toContain("Answer in the session's own terminal; this view is read-only.");
    expect(responses).toEqual([]);
    unmount();
  });
