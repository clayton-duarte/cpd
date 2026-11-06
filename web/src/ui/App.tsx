import { CircleAlertIcon, XIcon } from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactElement } from "react";
import type { ContextNode, PlanNode, SessionNode } from "../protocol/cockpit.ts";
import type { Scheduler, Transport } from "../protocol/runtime.ts";
import { createConnection } from "../state/connection.ts";
import { createStore, useStore } from "../state/store.ts";
import { BootstrapOverlay } from "./BootstrapOverlay.tsx";
import { Alert, AlertDescription, AlertTitle } from "./components/alert.tsx";
import { Button } from "./components/button.tsx";
import { routeErrorTarget } from "./errorReport.ts";
import { ErrorBoundary } from "./ErrorBoundary.tsx";
import type { CrashReport } from "../electron/health.ts";
import { activeLeadPlanId as leadPlanIdOf, loadSelection, saveSelection, visiblePlan as visiblePlanOf } from "./state/selection.ts";
import { ChatPane } from "./ChatPane.tsx";
import { ContextTree } from "./cockpit/ContextTree.tsx";
import { Header } from "./cockpit/Header.tsx";
import type { BreadcrumbCrumb } from "./cockpit/Header.tsx";
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

export type { FileReaderBoundary };
export interface AppProps { transport: Transport; scheduler: Scheduler; reader: FileReaderBoundary; newId?: () => string; onCrash?: (r: CrashReport) => void }

export function App(props: AppProps): ReactElement {
  const { transport, scheduler, reader, newId, onCrash } = props;
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
  const onDismissError = useCallback(() => store.dispatch({ type: "dismissError" }), [store]);
  const activeError = lastError ?? cockpit.error;
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
  const live = cockpit.tree !== null; // panes mount on the first snapshot, not before
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
  const dismissedUnread = useRef<Set<number>>(new Set());
  const inboxRows = cockpit.inbox?.rows;
  useEffect(() => {
    if (activeSessionId === null || inboxRows === undefined) return;
    for (const r of inboxRows) {
      if (r.kind !== "unread" || r.sessionId !== activeSessionId || r.id < 0 || dismissedUnread.current.has(r.id)) continue;
      dismissedUnread.current.add(r.id);
      onAction({ kind: "inbox.dismiss", itemId: r.id });
    }
  }, [activeSessionId, inboxRows, onAction]);
  const inboxModel = useMemo(() => {
    const inbox = cockpit.inbox;
    if (inbox === null || activeSessionId === null) return inbox;
    const rows = inbox.rows.filter((r) => !(r.kind === "unread" && (r.sessionId === activeSessionId || dismissedUnread.current.has(r.id))));
    return rows.length === inbox.rows.length ? inbox : { ...inbox, rows, openCount: rows.length };
  }, [cockpit.inbox, activeSessionId]);

  const crumbs = useMemo((): readonly BreadcrumbCrumb[] => {
    const tree = cockpit.tree;
    if (tree === null) {
      if (cockpit.plan === null) return [];
      return [
        { part: "manager", label: cockpit.plan.contextId },
        { part: "lead", label: cockpit.plan.planId },
      ];
    }
    const hostContext = tree.contexts.find((c) => c.id === "host");
    const hostManagerId =
      hostContext?.sessions.find((s) => s.ref.role === "manager" && !s.archived)?.ref.id ??
      hostContext?.sessions[0]?.ref.id;
    const activeSessionId = session?.id ?? selectedSessionId;
    let activeContext: ContextNode | undefined;
    let activePlan: PlanNode | undefined;
    let activeSession: SessionNode | undefined;

    if (activeSessionId !== null) {
      for (const c of tree.contexts) {
        const direct = c.sessions.find((s) => s.ref.id === activeSessionId);
        if (direct !== undefined) {
          activeContext = c;
          activeSession = direct;
          break;
        }
        for (const p of c.plans) {
          const inPlan = p.sessions.find((s) => s.ref.id === activeSessionId);
          if (inPlan !== undefined) {
            activeContext = c;
            activePlan = p;
            activeSession = inPlan;
            break;
          }
        }
        if (activeContext !== undefined) break;
      }
    }
    if (activeContext === undefined && cockpit.plan !== null) {
      activeContext = tree.contexts.find((c) => c.id === cockpit.plan?.contextId);
      activePlan = activeContext?.plans.find((p) => p.id === cockpit.plan?.planId);
    }

    const out: BreadcrumbCrumb[] = [];
    if (tree.hostname !== undefined || activeContext !== undefined) {
      out.push({
        part: "host",
        label: tree.hostname ?? "host",
        ...(hostManagerId !== undefined ? { onClick: () => onSelectSession(hostManagerId) } : {}),
      });
    }
    if (activeContext !== undefined) {
      const isStanding = STANDING_IDS.includes(activeContext.id);
      const isArchive =
        !isStanding &&
        (activeContext.archived || activePlan?.archived === true || activeSession?.archived === true);
      out.push({
        part: "group",
        label: isStanding ? "standing" : isArchive ? "archive" : "contexts",
      });
      const managerId =
        activeContext.sessions.find((s) => s.ref.role === "manager" && !s.archived)?.ref.id ??
        activeContext.sessions.find((s) => s.ref.role === "manager")?.ref.id;
      out.push({
        part: "manager",
        label: activeContext.id,
        ...(managerId !== undefined ? { onClick: () => onSelectSession(managerId) } : {}),
      });
      if (activePlan !== undefined) {
        const plan = activePlan;
        out.push({
          part: "lead",
          label: plan.id,
          onClick: () => {
            onSelectPlan(plan.id);
            if (plan.leadSessionId !== null) onSelectSession(plan.leadSessionId);
          },
        });
        if (activeSession !== undefined && activeSession.ref.role === "worker") {
          const workerId = activeSession.ref.id;
          out.push({
            part: "worker",
            label: activeSession.jobKey ?? activeSession.ref.key,
            onClick: () => onSelectSession(workerId),
          });
        } else if (activeSession !== undefined && activeSession.ref.id !== plan.leadSessionId) {
          const forkId = activeSession.ref.id;
          out.push({
            part: "worker",
            label: `${plan.id}·${forkId.slice(0, 4)}`,
            onClick: () => onSelectSession(forkId),
          });
        }
      } else if (activeSession !== undefined && activeSession.ref.role === "lead") {
        const forkId = activeSession.ref.id;
        out.push({
          part: "lead",
          label: activeSession.ref.key,
          onClick: () => onSelectSession(forkId),
        });
      }
    }
    return out;
  }, [cockpit.tree, cockpit.plan, session?.id, selectedSessionId, onSelectSession, onSelectPlan]);

  return (
    <>
    <BootstrapOverlay />
    {activeError === null ? null : (
      <div
        data-part="cockpit-error-toast"
        className="fixed right-4 bottom-8 z-50 w-full max-w-md shadow-lg"
      >
        <Alert variant="destructive" className="pr-9">
          <CircleAlertIcon className="size-4" />
          <AlertTitle>{activeError.code}</AlertTitle>
          <AlertDescription>{activeError.message}</AlertDescription>
          <Button
            type="button"
            variant="outline"
            size="sm"
            data-part="error-toast-report"
            className="mt-2"
            onClick={() => {
              onAction({ kind: "error.report", target: routeErrorTarget(activeError), code: activeError.code, message: activeError.message });
              onDismissError();
            }}
          >
            {`Send to ${routeErrorTarget(activeError)}`}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="Dismiss error"
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
      header={
        <Header
          crumbs={crumbs}
          location={cockpit.plan === null ? null : { context: cockpit.plan.contextId, plan: cockpit.plan.planId }}
          engine={cockpit.inbox === null ? null : cockpit.inbox.engineRunning}
        />
      }
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
            <MemoPlan plan={visiblePlan} {...(postableWorkflows !== null ? { postableWorkflows } : {})} onAction={onAction} />
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
      error={saved !== null && !saved.ok && !saving ? saved.error.message : null}
      onSave={onSaveSettings}
      onCancel={() => setSettingsOpen(false)}
    />
    </>
  );
}
