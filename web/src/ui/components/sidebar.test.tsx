// @vitest-environment jsdom
import { expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import {
  Sidebar, SidebarContent, SidebarGroup, SidebarGroupAction, SidebarGroupContent, SidebarGroupLabel,
  SidebarMenu, SidebarMenuAction, SidebarMenuButton, SidebarMenuItem, SidebarMenuSub, SidebarMenuSubButton, SidebarMenuSubItem,
} from "./sidebar.tsx";

it("sidebar parts render their documented hooks", () => {
  const m = mount(
    <Sidebar collapsible="none">
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Label</SidebarGroupLabel>
          <SidebarGroupAction aria-label="add">+</SidebarGroupAction>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton>Row</SidebarMenuButton>
                <SidebarMenuAction aria-label="toggle">&gt;</SidebarMenuAction>
                <SidebarMenuSub>
                  <SidebarMenuSubItem>
                    <SidebarMenuSubButton>Sub</SidebarMenuSubButton>
                  </SidebarMenuSubItem>
                </SidebarMenuSub>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
    </Sidebar>,
  );
  const c = m.container;
  for (const slot of [
    "sidebar", "sidebar-content", "sidebar-group", "sidebar-group-label", "sidebar-group-action", "sidebar-group-content",
    "sidebar-menu", "sidebar-menu-item", "sidebar-menu-button", "sidebar-menu-action", "sidebar-menu-sub", "sidebar-menu-sub-item", "sidebar-menu-sub-button",
  ]) {
    expect(c.querySelector(`[data-slot="${slot}"]`), slot).not.toBeNull();
  }
  expect(c.querySelector('[data-slot="sidebar-menu-button"]')?.tagName).toBe("BUTTON");
  expect(c.querySelector('[data-slot="sidebar-menu-sub"]')?.className).not.toContain("border-l");
  m.unmount();
});
