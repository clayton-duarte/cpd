// The app shell (docs/research/web-client-api.md §6): routing (react-router, hs-d770),
// layout, the dark/light theme and the token bootstrap. It also owns the held P1.6
// `ui_request`s and renders each one as an approval dialog answered via `ui_response`
// (§4.3, hs-d775; the four approval purposes of §1.2). The Escalation area elsewhere shows
// the count only (hs-d776).
import { useMemo, useState, type ReactElement } from "react";
import { NavLink, Route, Routes } from "react-router-dom";
import { uiResponsePost } from "../api/actions";
import type { LiveApi } from "../api/live";
import type { CpdApi } from "../api/mock";
import type { UiRequest } from "../api/types";
import type {
  InboxListResult,
  PlanListResult,
  PlanShowResult,
  SessionListResult,
} from "../api/readmodel";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { Chat, Inbox, Logs, Plan, Tree, type ChatFrame, type LogEntry } from "../views";
import { LiveChat } from "../views/live-chat";

/** The shell renders the current route and any held approval request. */
export interface AppProps {
  /** A held `ui_request` to show as an approval dialog, if any (§1.2 row 23). */
  uiRequest?: UiRequest;
  /** The typed API every action POSTs through, including `ui_response` (§4.4, hs-d775). */
  api?: CpdApi;
  /** The token every request carries (§4.2); absent when there is no socket. */
  token?: string;
  /**
   * The dev live adapter, built only when `main.tsx`'s `/__cpd/health` probe answers `ok`
   * (web/README.md "The UI wiring"); absent otherwise, so the mock's Chat renders.
   */
  live?: LiveApi;
}

// The empty-but-valid read-model slices the shell renders before the real reads land; the
// mirror shapes are the CLI's documented JSON (§3), so an empty read is always renderable.
const emptyPlanList: PlanListResult = { ok: true, plans: [], events_cursor: "" };
const emptyPlanShow: PlanShowResult = {
  ok: true,
  plan: {
    id: "plan-1",
    context: "ctx-1",
    title: "web-client",
    jira: null,
    phase: "active",
    lead_session: null,
    created_at: "",
    locked_at: null,
    done_at: null,
    archived_at: null,
    outline: null,
  },
  prs: [],
  jobs: [],
  escalations: null,
  review: null,
  counts: null,
  tamper: null,
  events_cursor: "",
};
const emptySessions: SessionListResult = { ok: true, sessions: [], events_cursor: "" };
const emptyInbox: InboxListResult = {
  ok: true,
  engine: { running: true },
  items: [],
  hidden: { snoozed: 0, dismissed: 0, claimed: 0 },
  events_cursor: "",
};
const emptyLog: readonly LogEntry[] = [];
const emptyFrames: readonly ChatFrame[] = [];

const routes: readonly [string, string][] = [
  ["/", "Tree"],
  ["/plan", "Plan"],
  ["/inbox", "Inbox"],
  ["/logs", "Logs"],
  ["/chat", "Chat"],
];

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

/** Renders the app shell with the held approval, if any. */
export function App({ uiRequest, api, token, live }: AppProps): ReactElement {
  const [dark, setDark] = useState(false);

  function toggleTheme(): void {
    setDark((value) => {
      const next = !value;
      if (typeof document !== "undefined") {
        document.documentElement.classList.toggle("dark", next);
      }
      return next;
    });
  }

  const tree = useMemo(
    () => ({ plans: emptyPlanList, sessions: emptySessions, inbox: emptyInbox }),
    [],
  );

  return (
    <div className="min-h-screen bg-background text-foreground">
      <header className="flex items-center gap-4 border-b p-3">
        <span className="font-semibold">cpd</span>
        <nav className="flex items-center gap-3 text-sm">
          {routes.map(([to, label]) => (
            <NavLink
              key={to}
              to={to}
              end={to === "/"}
              className={({ isActive }) => (isActive ? "text-primary underline" : "text-muted-foreground")}
            >
              {label}
            </NavLink>
          ))}
        </nav>
        <Button className="ml-auto" variant="outline" size="sm" onClick={toggleTheme}>
          {dark ? "light" : "dark"}
        </Button>
      </header>
      <main className="p-4">
        <Routes>
          <Route path="/" element={<Tree {...tree} />} />
          <Route path="/plan" element={<Plan plan={emptyPlanShow} />} />
          <Route path="/inbox" element={<Inbox inbox={emptyInbox} />} />
          <Route path="/logs" element={<Logs events={emptyLog} />} />
          <Route
            path="/chat"
            element={
              live !== undefined ? (
                <LiveChat api={live} token={token} />
              ) : (
                <Chat epoch="e" frames={emptyFrames} api={api} token={token} />
              )
            }
          />
        </Routes>
      </main>
      {uiRequest ? <ApprovalDialog request={uiRequest} api={api} token={token} /> : null}
    </div>
  );
}
