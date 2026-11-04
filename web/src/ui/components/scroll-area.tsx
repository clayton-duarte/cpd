import type { ReactElement, ReactNode } from "react";

/** Stub: the shadcn ScrollArea is not implemented yet. */
export function ScrollArea(props: { className?: string; children?: ReactNode }): ReactElement {
  return <div>{props.children}</div>;
}

export function ScrollBar(_props: { orientation?: "horizontal" | "vertical"; className?: string }): ReactElement {
  return <div />;
}
