// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { InboxModel, InboxRow } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { Inbox } from "./Inbox.tsx";

const prUrl = "https://github.com/acme/api/pull/3";

function dialog(id: number, url: string | null): InboxRow {
  return {
    id, kind: "dialog", label: "review dialog", urgency: "white", contextId: "codereview", planId: "p1", jobKey: null,
    sessionId: "lead1", url, workflow: null, openedAt: "2026-01-01T00:00:00Z", ageSeconds: 300, actions: [],
  };
}
function render(rows: InboxRow[]) {
  const model: InboxModel = { openCount: rows.length, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows, cursor: 1 };
  const onAction = vi.fn();
  const onSelectSession = vi.fn();
  const m = mount(<Inbox inbox={model} onAction={onAction} onSelectSession={onSelectSession} />);
  return { ...m, onAction, onSelectSession };
}
const items = (c: Element) => Array.from(c.querySelectorAll("[data-kind][data-id]"));
function click(el: Element | null | undefined): void {
  if (el === null || el === undefined) throw new Error("element not found");
  act(() => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}
function rightClick(el: Element | null | undefined): void {
  if (el === null || el === undefined) throw new Error("element not found");
  act(() => {
    el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 5, clientY: 5 }));
  });
}
const menuItems = () => Array.from(document.body.querySelectorAll('[role="menuitem"]'));
const menuLabels = () => menuItems().map((i) => i.textContent);

afterEach(() => {
  vi.unstubAllGlobals();
});

it("dialog item with url offers Open Link and Copy Link and keeps Go to Lead as primary click", () => {
  const writeText = vi.fn(async () => undefined);
  const open = vi.fn();
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  vi.stubGlobal("open", open);
  const m = render([dialog(1, prUrl)]);
  const [first] = items(m.container);
  click(first);
  expect(m.onSelectSession).toHaveBeenLastCalledWith("lead1");
  expect(open).not.toHaveBeenCalled();
  rightClick(first);
  expect(menuLabels()).toEqual(expect.arrayContaining(["Open Link", "Copy Link"]));
  click(menuItems().find((i) => i.textContent === "Open Link"));
  expect(open).toHaveBeenCalledWith(prUrl, "_blank", "noopener,noreferrer");
  rightClick(first);
  click(menuItems().find((i) => i.textContent === "Copy Link"));
  expect(writeText).toHaveBeenCalledWith(prUrl);
  m.unmount();
});

it("dialog item without url has no link actions", () => {
  vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn(async () => undefined) } });
  const m = render([dialog(1, prUrl), dialog(2, null)]);
  const [withUrl, without] = items(m.container);
  rightClick(withUrl);
  expect(menuLabels()).toContain("Open Link");
  act(() => {
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
  rightClick(without);
  expect(menuLabels()).not.toContain("Open Link");
  expect(menuLabels()).not.toContain("Copy Link");
  m.unmount();
});
