import type { ComponentProps } from "react";
import { cn } from "../../lib/utils.ts";

// shadcn/ui sidebar parts, stateless (no provider), on the Dango sidebar tokens.
export function Sidebar({ className, collapsible: _collapsible, ...p }: ComponentProps<"div"> & { collapsible?: "none" | "offcanvas" | "icon" }) {
  return <div data-slot="sidebar" data-sidebar="sidebar" className={cn("flex h-full w-full flex-col bg-sidebar text-sidebar-foreground", className)} {...p} />;
}
export function SidebarContent({ className, ...p }: ComponentProps<"div">) {
  return <div data-slot="sidebar-content" data-sidebar="content" className={cn("flex min-h-0 flex-1 flex-col gap-2 overflow-auto", className)} {...p} />;
}
export function SidebarGroup({ className, ...p }: ComponentProps<"div">) {
  return <div data-slot="sidebar-group" data-sidebar="group" className={cn("relative flex w-full min-w-0 flex-col p-1.5", className)} {...p} />;
}
export function SidebarGroupLabel({ className, ...p }: ComponentProps<"div">) {
  return <div data-slot="sidebar-group-label" data-sidebar="group-label" className={cn("flex h-8 shrink-0 items-center rounded-sm px-2 text-xs font-medium text-sidebar-foreground/70", className)} {...p} />;
}
export function SidebarGroupAction({ className, ...p }: ComponentProps<"button">) {
  return <button type="button" data-slot="sidebar-group-action" data-sidebar="group-action" className={cn("absolute right-3 top-3.5 flex aspect-square w-5 items-center justify-center rounded-md p-0 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground", className)} {...p} />;
}
export function SidebarGroupContent({ className, ...p }: ComponentProps<"div">) {
  return <div data-slot="sidebar-group-content" data-sidebar="group-content" className={cn("w-full text-sm", className)} {...p} />;
}
export function SidebarMenu({ className, ...p }: ComponentProps<"ul">) {
  return <ul data-slot="sidebar-menu" data-sidebar="menu" className={cn("flex w-full min-w-0 flex-col gap-0.5", className)} {...p} />;
}
export function SidebarMenuItem({ className, ...p }: ComponentProps<"li">) {
  return <li data-slot="sidebar-menu-item" data-sidebar="menu-item" className={cn("group/menu-item relative", className)} {...p} />;
}
export function SidebarMenuButton({ className, ...p }: ComponentProps<"button">) {
  return <button type="button" data-slot="sidebar-menu-button" data-sidebar="menu-button" className={cn("flex h-7 w-full items-center gap-2 overflow-hidden rounded-sm px-2 pr-8 text-left text-sm outline-none hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring", className)} {...p} />;
}
export function SidebarMenuAction({ className, ...p }: ComponentProps<"button">) {
  return <button type="button" data-slot="sidebar-menu-action" data-sidebar="menu-action" className={cn("absolute right-1 top-1 flex aspect-square w-5 items-center justify-center rounded-md p-0 outline-none hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring", className)} {...p} />;
}
export function SidebarMenuSub({ className, ...p }: ComponentProps<"ul">) {
  return <ul data-slot="sidebar-menu-sub" data-sidebar="menu-sub" className={cn("flex min-w-0 flex-col gap-0.5 pl-4", className)} {...p} />;
}
export function SidebarMenuSubItem({ className, ...p }: ComponentProps<"li">) {
  return <li data-slot="sidebar-menu-sub-item" data-sidebar="menu-sub-item" className={cn("group/menu-sub-item relative", className)} {...p} />;
}
export function SidebarMenuSubButton({ className, ...p }: ComponentProps<"button">) {
  return <button type="button" data-slot="sidebar-menu-sub-button" data-sidebar="menu-sub-button" className={cn("flex h-7 w-full min-w-0 items-center gap-2 overflow-hidden rounded-sm px-2 text-left text-sm outline-none hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring", className)} {...p} />;
}
