// @vitest-environment jsdom
import { act, createRef } from "react";
import { beforeEach, expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { resetLayout } from "./layout.ts";
import type { LayoutPanels } from "./layout.ts";
import { Shell } from "./Shell.tsx";

// Each resizable panel's `panelRef` is recorded by panel id, and every ref owns its own `resize` spy,
// so a double click is proven to resize exactly the one panel it names.
const { refs } = vi.hoisted(() => ({
  refs: new Map<string, { current: { resize: (size: string) => void } }>(),
}));

vi.mock("../components/resizable.tsx", async () => {
  const { createElement } = await import("react");
  const part = (kind: string) => (props: Record<string, unknown>) => {
    const { children, withHandle: _w, panelRef, onResize: _o, collapsible: _c, ...rest } = props;
    if (kind === "panel" && typeof rest["id"] === "string" && panelRef !== undefined) {
      refs.set(rest["id"], panelRef as { current: { resize: (size: string) => void } });
    }
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

const slots = { tree: <i>t</i>, plan: <i>p</i>, chat: <i>c</i>, inbox: <i>i</i> };
const resizeOf = (id: string) => refs.get(id)!.current.resize as ReturnType<typeof vi.fn>;
function dblclick(el: Element | null): void {
  act(() => {
    el?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
  });
}

beforeEach(() => refs.clear());

it("shell resets the contexts column to 20% on split-left double click", () => {
  const m = mount(<Shell {...slots} />);
  dblclick(m.container.querySelector("#split-left"));
  expect(resizeOf("tree")).toHaveBeenCalledExactlyOnceWith("20%");
  expect(resizeOf("inbox")).not.toHaveBeenCalled();
  expect(resizeOf("center")).not.toHaveBeenCalled();
  // pro-d50: the tree column never scrolls itself (its Sidebar content does)
  const aside = m.container.querySelector('aside[aria-label="Context tree column"]');
  expect(aside?.className).toContain("overflow-hidden");
  expect(aside?.className).not.toContain("overflow-y-auto");
  m.unmount();
});

it("shell resets the inbox column to 30% on split-right double click", () => {
  const panelsRef = createRef<LayoutPanels | null>() as { current: LayoutPanels | null };
  const m = mount(<Shell {...slots} panelsRef={panelsRef} />);
  dblclick(m.container.querySelector("#split-right"));
  expect(resizeOf("inbox")).toHaveBeenCalledExactlyOnceWith("30%");
  expect(resizeOf("tree")).not.toHaveBeenCalled();
  expect(resizeOf("center")).not.toHaveBeenCalled();
  // pro-n21: Reset layout restores the same columns, tree 20 / centre 50 / inbox 30, plan 40
  resizeOf("inbox").mockClear();
  resizeOf("tree").mockClear();
  resetLayout(panelsRef.current!);
  expect(resizeOf("tree")).toHaveBeenCalledExactlyOnceWith("20%");
  expect(resizeOf("center")).toHaveBeenCalledExactlyOnceWith("50%");
  expect(resizeOf("inbox")).toHaveBeenCalledExactlyOnceWith("30%");
  expect(resizeOf("plan")).toHaveBeenCalledExactlyOnceWith("40%");
  m.unmount();
});
