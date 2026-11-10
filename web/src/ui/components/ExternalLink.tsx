import type { ComponentProps, ReactElement } from "react";

export type ExternalLinkProps = Omit<ComponentProps<"a">, "target" | "rel">;

export function ExternalLink(props: ExternalLinkProps): ReactElement {
  // Stub for red tests: renders a plain anchor with neither target nor rel
  return <a {...props} />;
}
