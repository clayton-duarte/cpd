// The app root: the single-screen chat and nothing else (web/README.md "The chat surface
// (shadcn AI Elements)"). The shell header, the nav and the Tree/Plan/Inbox/Logs routes have
// left the render path; `ApprovalDialog` stays exported for the approval harness.
import type { ReactElement } from "react";
import { uiResponsePost } from "../api/actions";
import type { LiveApi } from "../api/live";
import type { CpdApi } from "../api/mock";
import type { UiRequest } from "../api/types";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { LiveChat } from "../views/live-chat";

/** The app root's props; the live adapter drives the one screen. */
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
 * Renders the single-screen chat and nothing else (web/README.md "The chat surface (shadcn AI
 * Elements)"): no shell header, no nav and no Tree/Plan/Inbox/Logs panes. The chat is the only
 * element on the screen; the live adapter is handed in when the probe answered `ok`.
 */
export function App({ token, live }: AppProps): ReactElement {
  return <LiveChat api={live} token={token} />;
}
