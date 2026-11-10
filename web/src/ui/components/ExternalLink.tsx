import type { ComponentProps, ReactElement } from "react";

export type ExternalLinkProps = Omit<ComponentProps<"a">, "target" | "rel">;

const EXTERNAL_SCHEMES = /^(https?:|mailto:)/i;

export function isExternalHref(href: string | undefined): boolean {
  if (!href) return false;
  return EXTERNAL_SCHEMES.test(href);
}

export function ExternalLink({ href, ...props }: ExternalLinkProps): ReactElement {
  const isExternal = isExternalHref(href);
  if (isExternal) {
    return <a href={href} target="_blank" rel="noopener noreferrer" {...props} />;
  }
  return <a href={href} {...props} />;
}

