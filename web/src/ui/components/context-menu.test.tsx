// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "./context-menu.tsx";

it("context menu parts render their documented hooks", () => {
  const onSelect = vi.fn();
  const m = mount(
    <ContextMenu>
      <ContextMenuTrigger>
        <p id="t">target</p>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={onSelect}>Dismiss</ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem>Copy Link</ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>,
  );
  const trigger = m.container.querySelector('[data-slot="context-menu-trigger"]');
  expect(trigger, "trigger slot").not.toBeNull();
  expect(trigger?.querySelector("#t"), "trigger child").not.toBeNull();
  act(() => {
    trigger?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 4, clientY: 4 }));
  });
  const content = document.body.querySelector('[data-slot="context-menu-content"]');
  expect(content, "content slot").not.toBeNull();
  const items = Array.from(content?.querySelectorAll('[data-slot="context-menu-item"]') ?? []);
  expect(items.map((i) => i.textContent), "item slots").toEqual(["Dismiss", "Copy Link"]);
  expect(items[0]?.getAttribute("role"), "item role").toBe("menuitem");
  expect(content?.querySelector('[data-slot="context-menu-separator"]'), "separator slot").not.toBeNull();
  act(() => {
    items[0]?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
  expect(onSelect, "item select").toHaveBeenCalledTimes(1);
  m.unmount();
});
