// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { InboxModel, InboxRow } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { Inbox } from "./Inbox.tsx";

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
});
afterEach(() => vi.unstubAllGlobals());

const base = { contextId: "c1", planId: "p1", jobKey: null, url: null, workflow: null, openedAt: "2026-01-01T00:00:00Z", ageSeconds: 300 };
function row(id: number, kind: string, label: string, over: Partial<InboxRow> = {}): InboxRow {
  return { ...base, id, kind, label, urgency: "white", sessionId: null, actions: [], ...over };
}
function render(rows: InboxRow[]) {
  const model: InboxModel = { openCount: rows.length, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows, cursor: 1 };
  return mount(<Inbox inbox={model} onAction={vi.fn()} onSelectSession={vi.fn()} />);
}

it("the inbox list scrolls with a ScrollArea", () => {
  const m = render([row(1, "unread", "one"), row(2, "unread", "two")]);
  const rows = Array.from(m.container.querySelectorAll("[data-kind][data-id]"));
  expect(rows.length).toBe(2);
  const area = m.container.querySelector('[data-slot="scroll-area"]');
  expect(area).not.toBeNull();
  expect(rows.every((r) => area?.contains(r) === true)).toBe(true);
  expect(m.container.querySelector(".overflow-y-auto")).toBeNull();
  expect(area?.closest('[data-part="inbox-header"]')).toBeNull();
  m.unmount();
});

it("the curate dialog body scrolls with a ScrollArea", () => {
  const m = render([
    row(1, "curate-retire", "Curate", {
      curation: [
        { id: "r1", detail: "d1", text: "t1" },
        { id: "r2", detail: "d2", text: "t2" },
      ],
    } as Partial<InboxRow>),
  ]);
  // the card click opens the owner; the Curate action button opens the dialog
  const curate = m.container.querySelector<HTMLButtonElement>('[data-kind][data-id] button[data-part="item-action"]');
  expect(curate?.textContent).toBe("Curate");
  act(() => {
    curate?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
  const box = document.body.querySelector('input[type="checkbox"]');
  expect(box).not.toBeNull();
  const area = box?.closest('[data-slot="scroll-area"]');
  expect(area).not.toBeNull();
  expect(area?.querySelector(".overflow-y-auto")).toBeNull();
  expect(box?.closest(".overflow-y-auto")).toBeNull();
  m.unmount();
});
