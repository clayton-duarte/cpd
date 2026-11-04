// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import type { InboxModel, InboxRow, Urgency } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { Inbox } from "./Inbox.tsx";

const base = { contextId: "c1", planId: "p1", jobKey: null, url: null, workflow: null, openedAt: "2026-01-01T00:00:00Z", ageSeconds: 5 };
const glyphs: Readonly<Record<Urgency, string>> = {
  red: "\u{F0362}",
  yellow: "\u{F069A}",
  magenta: "\u{F173A}",
  blue: "\u{F0368}",
  cyan: "\u{F06F1}",
  white: "\u{F0366}",
};

function render(rows: InboxRow[]) {
  const inbox: InboxModel = { openCount: rows.length, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows, cursor: 0 };
  return mount(<Inbox inbox={inbox} onAction={vi.fn()} onSelectSession={vi.fn()} />);
}

it("inbox urgency glyphs follow the TUI palette", () => {
  const urgencies = Object.keys(glyphs) as Urgency[];
  // kind "k" has no glyph of its own: the urgency picks it
  const m = render(urgencies.map((u, i) => ({ ...base, id: i + 1, kind: "k", label: `Row ${u}`, urgency: u, sessionId: `s${i}`, actions: [] })));
  const lis = Array.from(m.container.querySelectorAll("li[data-kind]"));
  expect(lis.map((li) => li.textContent?.startsWith(glyphs[li.getAttribute("data-urgency") as Urgency]))).toEqual(urgencies.map(() => true));
  m.unmount();
});
