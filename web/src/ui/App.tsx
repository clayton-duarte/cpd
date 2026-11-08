import { CircleAlertIcon, XIcon } from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactElement } from "react";
import type { ContextNode, PlanNode, SessionNode } from "../protocol/cockpit.ts";
import type { WireError } from "../protocol/domain.ts";
import type { Scheduler, Transport } from "../protocol/runtime.ts";
import { createConnection } from "../state/connection.ts";
import { createStore, useStore } from "../state/store.ts";
import { BootstrapOverlay } from "./BootstrapOverlay.tsx";
import { Alert, AlertDescription, AlertTitle } from "./components/alert.tsx";
import { Button } from "./components/button.tsx";
import { routeErrorTarget } from "./errorReport.ts";
import { ErrorBoundary } from "./ErrorBoundary.tsx";
import { HealthBeacon } from "./HealthBeacon.tsx";
import type { CrashReport } from "../electron/health.ts";
import { activeLeadPlanId as leadPlanIdOf, loadSelection, saveSelection, visiblePlan as visiblePlanOf } from "./state/selection.ts";
import { ChatPane } from "./ChatPane.tsx";
import { ContextTree } from "./cockpit/ContextTree.tsx";
import { Inbox } from "./cockpit/Inbox.tsx";
import { ManagerView } from "./cockpit/ManagerView.tsx";
import { PlanView } from "./cockpit/PlanView.tsx";
import { Shell } from "./cockpit/Shell.tsx";
import { barStatus, StatusBar } from "./cockpit/StatusBar.tsx";
import { resetLayout } from "./cockpit/layout.ts";
import type { LayoutPanels } from "./cockpit/layout.ts";
import type { SettingsChanges } from "../protocol/cockpit.ts";
import { DocsDialog } from "./dialogs/DocsDialog.tsx";
import { LegendDialog } from "./dialogs/LegendDialog.tsx";
import { LogsDialog } from "./dialogs/LogsDialog.tsx";
import { SettingsDialog } from "./dialogs/SettingsDialog.tsx";
import type { FileReaderBoundary } from "./composer/attachments.ts";

const MemoTree = memo(ContextTree);
const MemoPlan = memo(PlanView);
const MemoManager = memo(ManagerView);
const MemoInbox = memo(Inbox);

const STANDING_IDS: readonly string[] = ["cpd", "host", "codereview"];

const noErrors = (): number => 0;
/** The unread Inbox rows already hidden this session. Module-level, not a per-mount ref, so a remount does
 *  not send a second hide for a row whose item the engine has already closed (air-n11 B2). */
const dismissedUnread = new Set<number>();

export type { FileReaderBoundary };
export interface AppProps { transport: Transport; scheduler: Scheduler; reader: FileReaderBoundary; newId?: () => string; onCrash?: (r: CrashReport) => void; onHealthy?: () => void; errors?: () => number }

export function App(props: AppProps): ReactElement {
  const { transport, scheduler, reader, newId, onCrash, onHealthy, errors } = props;
  const crash = onCrash === undefined ? {} : { onCrash };
  const { store, connection } = useMemo(() => {
    const s = createStore();
    const c = createConnection({ transport, scheduler, store: s, ...(newId === undefined ? {} : { newId }) });
    return { store: s, connection: c };
  }, [transport, scheduler, newId]);
  useEffect(() => {
    connection.start();
    return () => connection.close();
  }, [connection]);
  const cockpit = useStore(store, (s) => s.cockpit);
  const session = useStore(store, (s) => s.session);
  const selectedSessionId = useStore(store, (s) => s.selectedSessionId);
  const lastError = useStore(store, (s) => s.lastError);
  const pendingActionIds = useStore(store, (s) => s.pendingActionIds);
  const completedActionIds = useStore(store, (s) => s.completedActionIds);
  const onSelectSession = useCallback((id: string) => connection.selectSession(id), [connection]);
  const onSelectPlan = useCallback((id: string) => connection.selectPlan(id), [connection]);
  const onAction = useCallback((a: Parameters<typeof connection.action>[0]) => connection.action(a), [connection]);
  // Keep the original error visible while its report is pending or has failed.
  const [report, setReport] = useState<{ id: string; error: WireError; failure: WireError | null } | null>(null);
  const reportInFlight = useRef(false);
  const [reportSent, setReportSent] = useState(false);
  const onDismissError = useCallback(() => { setReport(null); store.dispatch({ type: "dismissError" }); }, [store]);
  const sendReport = useCallback((error: WireError) => {
    if (reportInFlight.current) return;
    reportInFlight.current = true;
    setReportSent(false);
    const id = onAction({ kind: "error.report", target: routeErrorTarget(error), code: error.code, message: error.message });
    setReport({ id, error, failure: null });
  }, [onAction]);
  const activeError = lastError ?? cockpit.error;
  // While a report is in flight or failed the toast keeps its own error, so the send's failure (lastError)
  // never replaces it (air-d27).
  const shownError = report?.error ?? activeError;
  const panelsRef = useRef<LayoutPanels | null>(null);
  const [legendOpen, setLegendOpen] = useState(false);
  const [docsOpen, setDocsOpen] = useState(false);
  const [logsOpen, setLogsOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [costsOn, setCostsOn] = useState(false); // off at start, never persisted
  const costs = useMemo(() => {
    const flat = cockpit.costs ?? {};
    const contexts: Record<string, number> = {};
    const plans: Record<string, number> = {};
    for (const c of cockpit.tree?.contexts ?? []) {
      if (flat[c.id] !== undefined) contexts[c.id] = flat[c.id]!;
      for (const p of c.plans) if (flat[p.id] !== undefined) plans[p.id] = flat[p.id]!;
    }
    return { contexts, plans };
  }, [cockpit.costs, cockpit.tree]);
  const onReset = useCallback(() => { if (panelsRef.current !== null) resetLayout(panelsRef.current); }, []);
  const onLegend = useCallback(() => setLegendOpen(true), []);
  const onDocs = useCallback(() => setDocsOpen(true), []);
  const onLogs = useCallback(() => { connection.readLogs(); setLogsOpen(true); }, [connection]);
  const onSettings = useCallback(() => { connection.readSettings(); setSettingsOpen(true); }, [connection]);
  const onCosts = useCallback(() => setCostsOn((on) => !on), []);
  const onSaveSettings = useCallback((changes: SettingsChanges) => { setSaving(true); connection.saveSettings(changes); }, [connection]);
  const saved = cockpit.saved;
  useEffect(() => {
    if (!saving || saved === null) return;
    setSaving(false);
    if (saved.ok) setSettingsOpen(false);
  }, [saving, saved]);
  useEffect(() => {
    if (report === null || report.failure !== null) return;
    if (pendingActionIds.includes(report.id)) return;
    reportInFlight.current = false;
    if (completedActionIds.includes(report.id)) { onDismissError(); setReportSent(true); return; }
    setReport((r) => (r === null ? null : { ...r, failure: lastError ?? { code: "cpd_failed", message: "the report could not be sent" } }));
  }, [report, pendingActionIds, completedActionIds, lastError, onDismissError]);
  const live = cockpit.tree !== null; // panes mount on the first snapshot, not before
  // web/CONTRACT.md §15.2: the beacon reports health:ok once, only after the first cockpit.snapshot
  // (cockpit.tree goes from null to a tree); a painted shell with no data is not health.
  const sentinel = useRef<HTMLSpanElement>(null);
  const healthRoot = useCallback((): HTMLElement | null => {
    const parent = sentinel.current?.parentElement ?? null;
    if (parent === null) return null;
    return [...parent.children].some((child) => child !== sentinel.current) ? parent : null;
  }, []);
  const selectedPlanId = cockpit.selectedPlanId;
  const restored = useRef(false);
  useEffect(() => {
    if (!live) { restored.current = false; return; }
    if (restored.current) return;
    restored.current = true;
    const saved = loadSelection(window.sessionStorage);
    if (saved === null) return;
    connection.selectPlan(saved.planId);
    connection.selectSession(saved.sessionKey);
  }, [live, connection]);
  useEffect(() => {
    if (!live || !restored.current || selectedSessionId === null) return;
    saveSelection(window.sessionStorage, { planId: selectedPlanId, sessionKey: selectedSessionId });
  }, [live, selectedPlanId, selectedSessionId]);

  // pro-d63: an unread Inbox item of the chat being viewed is read: hide it at once and dismiss it once per id
  const activeSessionId = session?.id ?? null;
  const activeLeadPlanId = useMemo(() => leadPlanIdOf(cockpit.tree, activeSessionId), [cockpit.tree, activeSessionId]);
  useEffect(() => {
    if (!live || !restored.current || activeSessionId === null) return;
    if (selectedPlanId !== activeLeadPlanId) connection.selectPlan(activeLeadPlanId);
  }, [live, activeSessionId, activeLeadPlanId, selectedPlanId, connection]);
  const visiblePlan = useMemo(() => visiblePlanOf(activeLeadPlanId, cockpit.plan), [activeLeadPlanId, cockpit.plan]);
  const managerContext = useMemo(
    () => cockpit.tree?.contexts.find((c) => c.sessions.some((s) => s.ref.id === activeSessionId)) ?? null,
    [cockpit.tree, activeSessionId],
  );
  // Workflows whose PR the Inbox offers to post (a `pr.post` action) in the visible Plan
  const postableWorkflows = useMemo(() => {
    if (cockpit.inbox === null || visiblePlan === null) return null;
    const out = new Set<string>();
    for (const r of cockpit.inbox.rows) {
      if (r.planId !== visiblePlan.planId || !r.actions.some((a) => a.action.kind === "pr.post")) continue;
      if (r.workflow !== undefined && r.workflow !== null) out.add(r.workflow);
      if (r.url !== null) out.add(r.url);
      if (typeof r.subject === "string" && r.subject.includes("#")) out.add(`#${r.subject.split("#")[1]}`);
    }
    return out;
  }, [cockpit.inbox, visiblePlan]);
  const inboxRows = cockpit.inbox?.rows;
  useEffect(() => {
    if (activeSessionId === null || inboxRows === undefined) return;
    for (const r of inboxRows) {
      if (r.kind !== "unread" || r.sessionId !== activeSessionId || r.id < 0 || dismissedUnread.has(r.id)) continue;
      dismissedUnread.add(r.id);
      onAction({ kind: "inbox.dismiss", itemId: r.id });
    }
  }, [activeSessionId, inboxRows, onAction]);
  const inboxModel = useMemo(() => {
    const inbox = cockpit.inbox;
    if (inbox === null || activeSessionId === null) return inbox;
    const rows = inbox.rows.filter((r) => !(r.kind === "unread" && (r.sessionId === activeSessionId || dismissedUnread.has(r.id))));
    return rows.length === inbox.rows.length ? inbox : { ...inbox, rows, openCount: rows.length };
  }, [cockpit.inbox, activeSessionId]);

  return (
    <>
    <BootstrapOverlay />
    {reportSent ? (
      <div role="status" className="fixed right-4 bottom-8 z-50 rounded-lg border bg-card px-4 py-3 shadow-lg">
        Report sent
        <Button type="button" variant="ghost" size="icon-sm" aria-label="Dismiss report confirmation" onClick={() => setReportSent(false)}>
          <XIcon className="size-3.5" />
        </Button>
      </div>
    ) : null}
    {shownError === null ? null : (
      <div
        data-part="cockpit-error-toast"
        className="fixed right-4 bottom-8 z-50 w-full max-w-md shadow-lg"
      >
        <Alert variant="destructive" className="pr-9">
          <CircleAlertIcon className="size-4" />
          <AlertTitle>{shownError.code}</AlertTitle>
          <AlertDescription>{shownError.message}</AlertDescription>
          <Button
            type="button"
            variant="outline"
            size="sm"
            data-part="error-toast-report"
            className="mt-2 col-span-2 justify-self-start"
            disabled={report !== null}
            onClick={() => sendReport(shownError)}
          >
            {report !== null && report.failure === null ? "Sending…" : `Send to ${routeErrorTarget(shownError)}`}
          </Button>
          {report === null || report.failure === null ? null : (
            <div data-part="error-toast-failure" className="col-span-2 mt-1 flex items-center gap-2 text-xs">
              <span className="min-w-0 break-words">{`${report.failure.code}: ${report.failure.message}`}</span>
              <Button
                type="button"
                variant="outline"
                size="xs"
                data-part="error-toast-retry"
                onClick={() => sendReport(report.error)}
              >
                Retry
              </Button>
            </div>
          )}
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="Dismiss error"
            disabled={report !== null && report.failure === null}
            onClick={onDismissError}
            className="absolute top-2 right-2 size-6 text-destructive hover:text-destructive"
          >
            <XIcon className="size-3.5" />
          </Button>
        </Alert>
      </div>
    )}
    <Shell
      panelsRef={panelsRef}
      status={
        <StatusBar
          {...(cockpit.status === null ? { runner: session?.ref?.runner ?? null, state: session?.ref?.state ?? null, version: null } : { status: barStatus(cockpit.status, Date.now()) })}
          costsOn={costsOn}
          onReset={onReset}
          onLegend={onLegend}
          onDocs={onDocs}
          onLogs={onLogs}
          onCosts={onCosts}
          onSettings={onSettings}
        />
      }
      tree={
        !live ? null : <ErrorBoundary name="ContextTree" {...crash}><MemoTree
          tree={cockpit.tree}
          selectedSessionId={selectedSessionId}
          selectedPlanId={activeLeadPlanId}
          onSelectSession={onSelectSession}
          onSelectPlan={onSelectPlan}
          costsOn={costsOn}
          costs={costs}
          onAction={onAction}
        /></ErrorBoundary>
      }
      plan={
        !live ? null : managerContext !== null && visiblePlan === null ? (
          <ErrorBoundary name="ManagerView" {...crash}>
            <MemoManager context={managerContext} onSelectPlan={onSelectPlan} onSelectSession={onSelectSession} />
          </ErrorBoundary>
        ) : (
          <ErrorBoundary name="PlanView" {...crash}>
            <MemoPlan
              plan={visiblePlan}
              {...(postableWorkflows !== null ? { postableWorkflows } : {})}
              onAction={onAction}
              {...(visiblePlan === null ? {} : { onSelectManager: () => {
                const ctx = cockpit.tree?.contexts.find((c) => c.id === visiblePlan.contextId);
                const mgr = ctx?.sessions.find((x) => x.ref.role === "manager" && !x.archived);
                void connection.selectPlan(null);
                if (mgr !== undefined) onSelectSession(mgr.ref.id);
              } })}
            />
          </ErrorBoundary>
        )
      }
      chat={
        session === null ? (
          <p data-empty className="grid h-full place-items-center text-muted-foreground">Select a session</p>
        ) : (
          <ErrorBoundary name="ChatPane" {...crash}><ChatPane view={session} connection={connection} reader={reader} /></ErrorBoundary>
        )
      }
      inbox={
        !live ? null : <ErrorBoundary name="Inbox" {...crash}><MemoInbox
          inbox={inboxModel}
          onAction={onAction}
          onSelectSession={onSelectSession}
          onSelectPlan={onSelectPlan}
          pendingActionIds={pendingActionIds}
          completedActionIds={completedActionIds}
        /></ErrorBoundary>
      }
    />
    <LegendDialog open={legendOpen} onClose={() => setLegendOpen(false)} />
    <DocsDialog open={docsOpen} onClose={() => setDocsOpen(false)} />
    <LogsDialog open={logsOpen} onClose={() => setLogsOpen(false)} events={cockpit.logs ?? []} />
    <SettingsDialog
      open={settingsOpen}
      read={cockpit.settings}
      onSave={onSaveSettings}
      onCancel={() => setSettingsOpen(false)}
    />
    {onHealthy === undefined ? null : (
      <HealthBeacon live={live} root={healthRoot} errors={errors ?? noErrors} onHealthy={onHealthy} />
    )}
    <span ref={sentinel} hidden />
    </>
  );
}
