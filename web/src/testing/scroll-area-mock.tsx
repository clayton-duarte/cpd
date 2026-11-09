import { forwardRef } from "react";
import type { HTMLAttributes, ReactNode } from "react";

// Base UI measures layout in queued microtasks; jsdom has no layout. These tests exercise chat,
// not the external scrollbar, so keep the DOM boundary without running its measurement engine.
const part = (slot: string) => forwardRef<HTMLDivElement, HTMLAttributes<HTMLDivElement>>(function Part(props, ref) {
  return <div {...props} ref={ref} data-slot={slot} />;
});
const Root = part("scroll-area");
const Viewport = part("scroll-area-viewport");
const Content = part("scroll-area-content");
const Scrollbar = part("scroll-area-scrollbar");
const Thumb = part("scroll-area-thumb");
const Corner = part("scroll-area-corner");
export const ScrollArea = { Root, Viewport, Content, Scrollbar, Thumb, Corner };
export function ScrollAreaRoot({ children, ...props }: HTMLAttributes<HTMLDivElement> & { children?: ReactNode }) {
  return <Root {...props}>{children}</Root>;
}
export function ScrollBar() { return null; }
