import type { ComponentProps } from "react";
import type { Tone } from "../dango-status.ts";

export interface ButtonProps extends ComponentProps<"button"> {
  variant?: "default" | "outline" | "secondary" | "ghost" | "destructive" | "link";
  size?: "default" | "xs" | "sm" | "lg" | "icon" | "icon-xs" | "icon-sm" | "icon-lg";
  tone?: Tone;
}
/** Stub: plain button, no data-slot hooks. */
export function Button({ variant: _v, size: _s, tone: _t, ...props }: ButtonProps) {
  return <button {...props} />;
}
