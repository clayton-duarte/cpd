import { ChevronDownIcon, ChevronUpIcon, MaximizeIcon, MinimizeIcon } from "lucide-react";
import { useCallback, useState } from "react";
import type { ReactElement, ReactNode } from "react";
import { Group, Panel, Separator, usePanelRef } from "react-resizable-panels";
import { Button } from "../components/button.tsx";

type Mode = "open" | "collapsed" | "zoomed";
const CTRL = "size-[26px] rounded-[5px] text-fg-strong";

export function Shell(props: { tree: ReactNode; plan: ReactNode; chat: ReactNode; inbox: ReactNode; header?: ReactNode; status?: ReactNode }): ReactElement {
  const planRef = usePanelRef();
  const chatRef = usePanelRef();
  const [mode, setMode] = useState<Mode>("open");
  const sync = useCallback(() => {
    const planCollapsed = planRef.current?.isCollapsed() === true;
    const chatCollapsed = chatRef.current?.isCollapsed() === true;
    setMode(planCollapsed ? "collapsed" : chatCollapsed ? "zoomed" : "open");
  }, [planRef, chatRef]);
  const collapsed = mode === "collapsed";
  const zoomed = mode === "zoomed";
  return (
    <div data-shell className="bg-background font-sans text-body text-foreground">
      {props.header === undefined ? null : <header aria-label="Cockpit header" className="bg-background">{props.header}</header>}
      <aside aria-label="Context tree column" className="border-r border-border">{props.tree}</aside>
      <div data-column="center" data-part="center-frame" data-plan-state={mode} className="relative flex h-full min-h-0 flex-col">
        <Group data-part="center-group" orientation="vertical" id="cpd-center" className="h-full min-h-0 flex-1">
          <Panel
            id="plan"
            data-part="plan-panel"
            panelRef={planRef}
            defaultSize="40%"
            minSize="20%"
            maxSize="80%"
            collapsible
            collapsedSize="44px"
            onResize={sync}
            className="min-h-0"
          >
            <section aria-label="Plan pane" data-state={mode} {...(collapsed ? { inert: true } : {})}>{props.plan}</section>
          </Panel>
          <Separator
            id="split"
            aria-label="Resize plan view and chat"
            data-part="split-handle"
            className="grid h-[9px] cursor-row-resize place-items-center border-t border-border"
          >
            <span aria-hidden="true" className="h-[3px] w-9 rounded-[2px] bg-input" />
          </Separator>
          <Panel id="chat" data-part="chat-panel" panelRef={chatRef} defaultSize="60%" minSize="20%" collapsible collapsedSize="0%" onResize={sync}>
            <main aria-label="Chat" data-state={zoomed ? "zoomed" : "open"} className="h-full min-h-0" {...(zoomed ? { inert: true } : {})}>{props.chat}</main>
          </Panel>
        </Group>
        <div data-part="plan-controls" role="group" aria-label="Plan pane controls" className="absolute right-4 top-[9px] z-10 flex gap-0.5">
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            data-action="plan-collapse"
            className={CTRL}
            aria-label={collapsed ? "Expand plan view" : "Collapse plan view"}
            disabled={zoomed}
            onClick={() => (collapsed ? planRef.current?.expand() : planRef.current?.collapse())}
          >
            {collapsed ? <ChevronDownIcon size={14} /> : <ChevronUpIcon size={14} />}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            data-action="plan-zoom"
            className={CTRL}
            aria-label="Zoom plan view"
            aria-pressed={zoomed}
            disabled={collapsed}
            onClick={() => (zoomed ? chatRef.current?.expand() : chatRef.current?.collapse())}
          >
            {zoomed ? <MinimizeIcon size={14} /> : <MaximizeIcon size={14} />}
          </Button>
        </div>
      </div>
      <aside aria-label="Inbox column" className="border-l border-border">{props.inbox}</aside>
      {props.status === undefined ? null : <footer aria-label="Status bar" className="flex items-center border-t border-border bg-surface-sunken">{props.status}</footer>}
    </div>
  );
}
