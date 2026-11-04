import type { ComponentProps } from "react";

export interface MarkerProps extends ComponentProps<"div"> {
  variant?: "default" | "separator" | "border";
}
/** Stub: plain div, no data-slot hooks. */
export function Marker({ variant: _v, ...props }: MarkerProps) {
  return <div {...props} />;
}
export function MarkerContent(props: ComponentProps<"span">) {
  return <span {...props} />;
}
