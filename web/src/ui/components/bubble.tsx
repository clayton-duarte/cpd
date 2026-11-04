import type { ComponentProps } from "react";

export interface BubbleProps extends ComponentProps<"div"> {
  variant?: "default" | "secondary" | "muted" | "tinted" | "outline" | "ghost" | "destructive";
  align?: "start" | "end";
}
/** Stub: plain div, no data-slot hooks. */
export function Bubble({ variant: _v, align: _a, ...props }: BubbleProps) {
  return <div {...props} />;
}
export function BubbleContent(props: ComponentProps<"div">) {
  return <div {...props} />;
}
