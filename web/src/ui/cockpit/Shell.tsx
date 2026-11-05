import { ChevronDownIcon, ChevronUpIcon, MaximizeIcon, MinimizeIcon } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import type { MutableRefObject, ReactElement, ReactNode } from "react";
import { Button } from "../components/button.tsx";
import type { LayoutPanels } from "./layout.ts";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
  usePanelRef,
} from "../components/resizable.tsx";

type Mode = "open" | "collapsed" | "zoomed";
const CTRL = "size-[26px] rounded-[5px] text-fg-strong";

export function Shell(props: {
  tree: ReactNode;
  plan: ReactNode;
  chat: ReactNode;
  inbox: ReactNode;
  header?: ReactNode;
  status?: ReactNode;
  /** Filled with the resizable panels, for `resetLayout`. */
  panelsRef?: MutableRefObject<LayoutPanels | null>;
}): ReactElement {
  const planRef = usePanelRef();
  const treeRef = usePanelRef();
  const centerRef = usePanelRef();
  const inboxRef = usePanelRef();
  const { panelsRef } = props;
  useEffect(() => {
    if (panelsRef === undefined) return;
    const api = (ref: typeof planRef) => ({ resize: (size: string) => ref.current?.resize(size) });
    panelsRef.current = { tree: api(treeRef), center: api(centerRef), inbox: api(inboxRef), plan: api(planRef) };
    return () => { panelsRef.current = null; };
  }, [panelsRef, treeRef, centerRef, inboxRef, planRef]);
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
      {props.header === undefined ? null : (
        <header aria-label="Cockpit header" className="bg-background">
          {props.header}
        </header>
      )}
      <ResizablePanelGroup
        id="cpd-columns"
        data-part="columns-group"
        orientation="horizontal"
        className="min-h-0 flex-1"
      >
        <ResizablePanel
          id="tree"
          panelRef={treeRef}
          data-part="tree-panel"
          defaultSize="20%"
          minSize="12%"
          maxSize="40%"
          collapsible
          collapsedSize="0%"
        >
          <aside aria-label="Context tree column" className="h-full overflow-y-auto">
            {props.tree}
          </aside>
        </ResizablePanel>
        <ResizableHandle
          id="split-left"
          withHandle
          aria-label="Resize context tree and center"
          data-part="split-left"
        />
        <ResizablePanel id="center" panelRef={centerRef} data-part="center-panel" defaultSize="58%" minSize="30%">
          <div
            data-column="center"
            data-part="center-frame"
            data-plan-state={mode}
            className="relative flex h-full min-h-0 flex-col"
          >
            <ResizablePanelGroup
              data-part="center-group"
              orientation="vertical"
              id="cpd-center"
              className="h-full min-h-0 flex-1"
            >
              <ResizablePanel
                id="plan"
                data-part="plan-panel"
                panelRef={planRef}
                defaultSize="40%"
                minSize="20%"
                maxSize="80%"
                collapsible
                collapsedSize="44px"
                onResize={sync}
              >
                <section aria-label="Plan pane" data-state={mode} {...(collapsed ? { inert: true } : {})}>
                  {props.plan}
                </section>
              </ResizablePanel>
              <ResizableHandle
                id="split"
                withHandle
                aria-label="Resize plan view and chat"
                data-part="split-handle"
              />
              <ResizablePanel
                id="chat"
                data-part="chat-panel"
                panelRef={chatRef}
                defaultSize="60%"
                minSize="20%"
                collapsible
                collapsedSize="0%"
                onResize={sync}
              >
                <main
                  aria-label="Chat"
                  data-state={zoomed ? "zoomed" : "open"}
                  className="h-full min-h-0"
                  {...(zoomed ? { inert: true } : {})}
                >
                  {props.chat}
                </main>
              </ResizablePanel>
            </ResizablePanelGroup>
            <div
              data-part="plan-controls"
              role="group"
              aria-label="Plan pane controls"
              className="absolute right-4 top-[9px] z-10 flex gap-0.5"
            >
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
        </ResizablePanel>
        <ResizableHandle
          id="split-right"
          withHandle
          aria-label="Resize center and inbox"
          data-part="split-right"
        />
        <ResizablePanel
          id="inbox"
          panelRef={inboxRef}
          data-part="inbox-panel"
          defaultSize="22%"
          minSize="14%"
          maxSize="45%"
          collapsible
          collapsedSize="0%"
        >
          <aside aria-label="Inbox column" className="h-full overflow-y-auto">
            {props.inbox}
          </aside>
        </ResizablePanel>
      </ResizablePanelGroup>
      {props.status === undefined ? null : (
        <footer aria-label="Status bar" className="flex items-center border-t border-border bg-surface-sunken">
          {props.status}
        </footer>
      )}
    </div>
  );
}
