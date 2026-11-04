import type { ReactElement } from "react";

export interface HeaderProps {
  readonly location?: { readonly context: string; readonly plan: string | null } | null;
  readonly engine?: boolean | null;
  readonly onOpenSearch?: () => void;
}

// Stub: the implementation Worker replaces this with the DESIGN.md 3a markup.
export function Header(_props: HeaderProps): ReactElement {
  return <strong data-emblem>C-P-D</strong>;
}
