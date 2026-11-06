// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
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

vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

const slots = { tree: <i>t</i>, plan: <i id="p">p</i>, chat: <i>c</i>, inbox: <i>i</i> };

it("the plan pane wraps its content in a ScrollArea", () => {
  const m = mount(<Shell {...slots} />);
  expect(m.container.querySelector('section[aria-label="Plan pane"] [data-slot="scroll-area"] #p')).not.toBeNull();
  m.unmount();
});

it("the inbox column does not scroll itself", () => {
  const m = mount(<Shell {...slots} />);
  const aside = m.container.querySelector('aside[aria-label="Inbox column"]');
  expect(aside).not.toBeNull();
  expect(aside?.className).toContain("overflow-hidden");
  expect(aside?.className).not.toContain("overflow-y-auto");
  m.unmount();
});
