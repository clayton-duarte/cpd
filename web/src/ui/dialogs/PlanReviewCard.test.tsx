// @vitest-environment jsdom
import { act } from "react";
import { expect, it } from "vitest";
import type { Dialog, DialogResponse } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { PlanReviewCard } from "./PlanReviewCard.tsx";

function setup(readOnly = false) {
  const responses: DialogResponse[] = [];
  const dialog: Extract<Dialog, { method: "cpd_plan_review" }> = {
    id: "review:r1",
    method: "cpd_plan_review",
    interactive: !readOnly,
    title: "Ship the widget",
    summary: "Three **bold** steps",
  };
  const view = mount(<PlanReviewCard dialog={dialog} readOnly={readOnly} onRespond={(r) => responses.push(r)} />);
  return { ...view, responses };
}

function btn(c: HTMLElement, action: string): HTMLButtonElement {
  const b = c.querySelector<HTMLButtonElement>(`button[data-action="${action}"]`);
  if (!b) throw new Error(`no button ${action}`);
  return b;
}

  it("shows the plan title and summary", () => {
    const { container, unmount } = setup();
    expect(container.querySelector('section[aria-label="Plan review"][data-dialog="cpd_plan_review"]')).not.toBeNull();
    expect(container.querySelector("h3")?.textContent).toBe("Ship the widget");
    expect(container.textContent).toContain("Three bold steps");
    expect(container.querySelector("strong")?.textContent).toBe("bold");
    unmount();
  });

  it("Approve responds approve", () => {
    const { container, responses, unmount } = setup();
    act(() => btn(container, "approve").click());
    expect(responses).toEqual([{ method: "cpd_plan_review", outcome: "approve" }]);
    unmount();
  });

  it("Request changes requires feedback text", () => {
    const { container, responses, unmount } = setup();
    expect(container.querySelector("textarea")).toBeNull();
    act(() => btn(container, "request-changes").click());
    const area = container.querySelector('textarea[aria-label="Feedback"]') as HTMLTextAreaElement;
    expect(area).not.toBeNull();
    const send = btn(container, "send-feedback");
    expect(send.disabled).toBe(true);
    const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
    const write = (v: string) =>
      act(() => {
        set?.call(area, v);
        area.dispatchEvent(new Event("input", { bubbles: true }));
      });
    write("   ");
    expect(btn(container, "send-feedback").disabled).toBe(true);
    write("  tighten scope  ");
    expect(btn(container, "send-feedback").disabled).toBe(false);
    act(() => btn(container, "send-feedback").click());
    expect(responses).toEqual([{ method: "cpd_plan_review", outcome: "changes", feedback: "tighten scope" }]);
    unmount();
  });

  it("Reject responds reject", () => {
    const { container, responses, unmount } = setup();
    act(() => btn(container, "reject").click());
    expect(responses).toEqual([{ method: "cpd_plan_review", outcome: "reject" }]);
    unmount();
  });

  it("read-only mode renders the review without controls", () => {
    const { container, responses, unmount } = setup(true);
    expect(container.textContent).toContain("Ship the widget");
    expect(container.textContent).toContain("Three bold steps");
    expect(container.querySelector("button, textarea, input")).toBeNull();
    expect(responses).toEqual([]);
    unmount();
  });
