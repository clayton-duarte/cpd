import type { ComponentProps, ReactElement } from "react";

export type ExternalLinkProps = Omit<ComponentProps<"a">, "target" | "rel">;

export function ExternalLink(_props: ExternalLinkProps): ReactElement {
  // Stub for red tests: throws or returns empty stub until implemented in web-impl
  throw new Error("ExternalLink: stub not implemented");
}
