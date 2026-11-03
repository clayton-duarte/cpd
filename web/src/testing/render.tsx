import type { ReactElement } from "react";

export function mount(_element: ReactElement): { container: HTMLElement; unmount(): void } {
  throw new Error("not implemented");
}
