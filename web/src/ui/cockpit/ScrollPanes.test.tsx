// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ContextTreeModel, InboxModel, InboxRow } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { ContextTree } from "./ContextTree.tsx";
import { Inbox } from "./Inbox.tsx";
import { Shell } from "./Shell.tsx";

vi.mock("../components/resizable.tsx", async () => {
  const { createElement } = await import("react");
  const part = (kind: string) => (props: Record<string, unknown>) => {
    const { children, withHandle: _w, panelRef, onResize: _o, collapsible: _c, ...rest } = props;
    void panelRef;
    return createElement("div", { "data-kind": kind, ...(rest as object) }, children as never);
  };
  return {
    ResizablePanelGroup: part("group"),
    ResizablePanel: part("panel"),
    ResizableHandle: part("handle"),
    usePanelRef: () => ({ current: { resize: vi.fn(), isCollapsed: () => false, collapse: vi.fn(), expand: vi.fn() } }),
  };
});

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
});
afterEach(() => vi.unstubAllGlobals());

const W = "[&>[data-slot=scroll-area-viewport]>[data-slot=scroll-area-content]]:w-full";
const MIN = "[&>[data-slot=scroll-area-viewport]>[data-slot=scroll-area-content]]:!min-w-0";

function expectPane(area: Element | null | undefined, part: string): void {
  expect(area).not.toBeNull();
  expect(area?.getAttribute("data-part")).toBe(part);
  const cls = area?.className ?? "";
  expect(cls).toContain(W);
  expect(cls).toContain(MIN);
}

it("Shell Plan pane scroll area is plan-scroll and keeps its content at pane width", () => {
  const m = mount(<Shell tree={<i>t</i>} plan={<i id="p">p</i>} chat={<i>c</i>} inbox={<i>i</i>} />);
  const kid = m.container.querySelector('section[aria-label="Plan pane"] #p');
  expectPane(kid?.closest("[data-slot=scroll-area]"), "plan-scroll");
  m.unmount();
});

it("ContextTree section scroll area is tree-scroll and keeps its content at pane width", () => {
  const tree: ContextTreeModel = {
    cursor: 1,
    contexts: [{ id: "c1", title: "Context One", archived: false, records: 0, openQuestions: 0, sessions: [], plans: [] }],
  };
  const m = mount(
    <ContextTree tree={tree} selectedSessionId={null} selectedPlanId={null} onSelectSession={vi.fn()} onSelectPlan={vi.fn()} />,
  );
  expectPane(m.container.querySelector("[data-slot=scroll-area]"), "tree-scroll");
  m.unmount();
});

it("Inbox row list scroll area is inbox-scroll and keeps its content at pane width", () => {
  const base = { contextId: "c1", planId: "p1", jobKey: null, url: null, workflow: null, openedAt: "2026-01-01T00:00:00Z", ageSeconds: 300 };
  const rows: InboxRow[] = [1, 2].map((id) => ({ ...base, id, kind: "unread", label: `r${id}`, urgency: "white", sessionId: null, actions: [] }));
  const model: InboxModel = { openCount: 2, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows, cursor: 1 };
  const m = mount(<Inbox inbox={model} onAction={vi.fn()} onSelectSession={vi.fn()} />);
  const row = m.container.querySelector("[data-kind][data-id]");
  expectPane(row?.closest("[data-slot=scroll-area]"), "inbox-scroll");
  m.unmount();
});
