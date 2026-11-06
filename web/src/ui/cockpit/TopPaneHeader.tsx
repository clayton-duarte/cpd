import type { ReactElement, ReactNode, Ref } from "react";
import { cn } from "../../lib/utils.ts";

/** The shared sticky h-11 header of the top panes: `<children> ___ <actions>`. */
export function TopPaneHeader(props: {
  dataPart?: string;
  headerRef?: Ref<HTMLElement>;
  hasOverlayControls?: boolean;
  actions?: ReactNode;
  children?: ReactNode;
}): ReactElement {
  const { dataPart = "pane-header", headerRef, hasOverlayControls = true, actions, children } = props;
  return (
    <header
      ref={headerRef}
      data-part={dataPart}
      className={cn(
        "sticky top-0 z-10 flex h-11 shrink-0 items-center gap-2.5 border-b border-border bg-background px-4",
        hasOverlayControls && "pr-12",
      )}
    >
      {children}
      <span className="flex-1" />
      {actions}
    </header>
  );
}
