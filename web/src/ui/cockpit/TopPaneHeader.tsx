import { ChevronDownIcon, ChevronUpIcon } from "lucide-react";
import { useContext } from "react";
import type { ReactElement, ReactNode, Ref } from "react";
import { cn } from "../../lib/utils.ts";
import { Button } from "../components/button.tsx";
import { PlanCollapseContext } from "./Shell.tsx";

/** The shared h-11 header of the top panes: `<children> ___ <actions> [plan collapse]`. */
export function TopPaneHeader(props: {
  dataPart?: string;
  headerRef?: Ref<HTMLElement>;
  /** Show the plan pane collapse control (needs a PlanCollapseContext). */
  hasOverlayControls?: boolean;
  actions?: ReactNode;
  children?: ReactNode;
}): ReactElement {
  const { dataPart = "pane-header", headerRef, hasOverlayControls = true, actions, children } = props;
  const plan = useContext(PlanCollapseContext);
  return (
    <header
      ref={headerRef}
      data-part={dataPart}
      className={cn("flex h-11 shrink-0 items-center gap-2.5 border-b border-border bg-background px-4")}
    >
      {children}
      <span className="flex-1" />
      {actions}
      {hasOverlayControls && plan !== null ? (
        <div data-part="plan-controls" role="group" aria-label="Plan pane controls" className="flex shrink-0 items-center gap-0.5">
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            data-action="plan-collapse"
            className="size-[26px] rounded-[5px] text-fg-strong"
            aria-label={plan.collapsed ? "Expand plan view" : "Collapse plan view"}
            onClick={plan.toggle}
          >
            {plan.collapsed ? <ChevronDownIcon size={14} /> : <ChevronUpIcon size={14} />}
          </Button>
        </div>
      ) : null}
    </header>
  );
}
