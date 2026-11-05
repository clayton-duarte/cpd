// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import type { InboxModel, InboxRow, Urgency } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { Inbox } from "./Inbox.tsx";

const base = { contextId: "c1", planId: "p1", jobKey: null, url: null, workflow: null, openedAt: "2026-01-01T00:00:00Z", ageSeconds: 5 };
const palette: Readonly<Record<Urgency, [string, string]>> = {
  red: ["text-status-blocked", "lucide-circle-alert"],
  yellow: ["text-status-landing", "lucide-circle-dot"],
  magenta: ["text-status-decision", "lucide-circle-question-mark"],
  blue: ["text-status-focus", "lucide-eye"],
  cyan: ["text-status-active", "lucide-bell"],
  white: ["text-muted-foreground", "lucide-message-square"],
};

function render(rows: InboxRow[]) {
  const inbox: InboxModel = { openCount: rows.length, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows, cursor: 0 };
  return mount(<Inbox inbox={inbox} onAction={vi.fn()} onSelectSession={vi.fn()} />);
}

it("inbox urgency icons follow the status palette", () => {
  const urgencies = Object.keys(palette) as Urgency[];
  const m = render(urgencies.map((u, i) => ({ ...base, id: i + 1, kind: "k", label: `Row ${u}`, urgency: u, sessionId: `s${i}`, actions: [] })));
  const rows = Array.from(m.container.querySelectorAll("a[data-kind]"));
  expect(rows).toHaveLength(urgencies.length);
  for (const el of rows) {
    const [text, icon] = palette[el.getAttribute("data-urgency") as Urgency];
    expect(el.querySelector('[data-slot="item-title"]')?.className.split(" ")).toContain(text);
    expect(el.querySelector('[data-slot="item-media"] svg')?.getAttribute("class")?.split(" ")).toContain(icon);
  }
  m.unmount();
});
