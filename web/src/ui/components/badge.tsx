import type { ComponentProps } from "react";
import type { Tone } from "../dango-status.ts";

export interface BadgeProps extends ComponentProps<"span"> {
  variant?: "default" | "secondary" | "destructive" | "outline" | "ghost" | "link";
  tone?: Tone;
}
/** Stub: plain span, no data-slot hooks. */
export function Badge({ variant: _v, tone: _t, ...props }: BadgeProps) {
  return <span {...props} />;
}
