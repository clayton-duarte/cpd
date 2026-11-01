// The app root: the resizable three-column shell Context | Chat | Inbox (web/README.md "The
// resizable three-column shell"). The Context column is the vendored shadcn `sidebar-03` block,
// the centre column is the existing Live Chat unchanged, and the Inbox is a placeholder (the
// real Inbox and the Tree/Plan panes belong to the ui-parity Plan). Sizing is
// `react-resizable-panels` 4.15.3 through the vendored `resizable` wrapper; the Context column
// defaults to 20rem and its handle restores that on double-click. `ApprovalDialog` stays
// exported for the approval harness.
import { useEffect, type CSSProperties, type ReactElement } from "react";
import { usePanelRef } from "react-resizable-panels";
import { uiResponsePost } from "../api/actions";
import type { LiveApi } from "../api/live";
import type { CpdApi } from "../api/mock";
import type { UiRequest } from "../api/types";
import { AppSidebar } from "../components/app-sidebar";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "../components/ui/resizable";
import { SidebarProvider } from "../components/ui/sidebar";
import { LiveChat } from "../views/live-chat";

/** The Context column's default width (web/README.md "Sizing"); the handle restores it. */
const CONTEXT_DEFAULT = "20rem";

/** The app root's props; the live adapter drives the Chat column. */
export interface AppProps {
  /** A held `ui_request` for the approval dialog (kept for the approval harness). */
  uiRequest?: UiRequest;
  /** The typed mock API (kept for the approval harness; no longer rendered here). */
  api?: CpdApi;
  /** The token every request carries (§4.2); absent when there is no socket. */
  token?: string;
  /**
   * The dev live adapter, built only when `main.tsx`'s `/__cpd/health` probe answers `ok`
   * (web/README.md "The UI wiring"); absent otherwise, so the explicit no-adapter state shows.
   */
  live?: LiveApi;
}

/** One approval `ui_request` as a dialog listing its options (hs-d775). */
export function ApprovalDialog({
  request,
  api,
  token,
}: {
  request: UiRequest;
  api?: CpdApi;
  token?: string;
}): ReactElement {
  function answer(payload: { value?: string; confirmed?: boolean; cancelled?: true }): void {
    if (api === undefined || token === undefined) return;
    api.post(
      uiResponsePost({ type: "ui_response", req: request.tool_call_id ?? request.id, id: request.id, ...payload }),
      { token },
    );
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      data-slot="approval-dialog"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
    >
      <Card className="w-full max-w-lg">
        <CardHeader>
          <CardTitle>{request.title ?? "Approval requested"}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {request.message ? <p className="text-sm text-muted-foreground">{request.message}</p> : null}
          <div className="flex flex-wrap gap-2">
            {(request.options ?? []).map((option) => (
              <Button key={option.label} onClick={() => answer({ value: option.label })}>
                {option.label}
              </Button>
            ))}
            {request.method === "confirm" ? (
              <Button onClick={() => answer({ confirmed: true })}>Confirm</Button>
            ) : null}
            <Button variant="outline" onClick={() => answer({ cancelled: true })}>
              Cancel
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

/**
 * The Context (left) column: the vendored `sidebar-03` block. Its desktop rail is
 * `position: fixed`, so it needs a `transform`ed containing block and `--sidebar-width: 100%`
 * so the rail and its gap spacer match the panel without adding a panel scrollbar
 * (web/README.md "Containing the fixed rail").
 */
function ContextColumn(): ReactElement {
  return (
    <div className="h-full overflow-hidden [transform:translateZ(0)]">
      <SidebarProvider
        style={{ "--sidebar-width": "100%" } as CSSProperties}
        className="min-h-0"
      >
        <AppSidebar />
      </SidebarProvider>
    </div>
  );
}

/** The Inbox (right) column: a placeholder; the real Inbox belongs to the ui-parity Plan. */
function InboxColumn(): ReactElement {
  return (
    <div className="flex h-full items-center justify-center p-6 text-sm text-muted-foreground">
      The Inbox lands here.
    </div>
  );
}

/**
 * Renders the resizable three-column shell (web/README.md "The resizable three-column shell"):
 * Context | Chat | Inbox, with a handle between each. The Context column defaults to 20rem; the
 * three panels' default sizes over-constrain the group, so the mount effect re-applies 20rem
 * through the panel ref to land it exactly, and the Context handle's double-click restores it.
 */
export function App({ token, live }: AppProps): ReactElement {
  const contextPanel = usePanelRef();

  useEffect(() => {
    contextPanel.current?.resize(CONTEXT_DEFAULT);
  }, [contextPanel]);

  return (
    <ResizablePanelGroup orientation="horizontal" className="h-svh">
      <ResizablePanel
        defaultSize={CONTEXT_DEFAULT}
        panelRef={contextPanel}
        className="min-w-0"
      >
        <ContextColumn />
      </ResizablePanel>
      <ResizableHandle
        disableDoubleClick
        onDoubleClick={() => contextPanel.current?.resize(CONTEXT_DEFAULT)}
      />
      <ResizablePanel defaultSize="60%" className="min-w-0">
        <LiveChat api={live} token={token} />
      </ResizablePanel>
      <ResizableHandle disableDoubleClick />
      <ResizablePanel defaultSize="20%" className="min-w-0">
        <InboxColumn />
      </ResizablePanel>
    </ResizablePanelGroup>
  );
}
