import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactElement } from "react";
import type { Scheduler, Transport } from "../protocol/runtime.ts";
import { createConnection } from "../state/connection.ts";
import { createStore, useStore } from "../state/store.ts";
import { BootstrapOverlay } from "./BootstrapOverlay.tsx";
import { loadSelection, saveSelection } from "./state/selection.ts";
import { ChatPane } from "./ChatPane.tsx";
import { ContextTree } from "./cockpit/ContextTree.tsx";
import { Header } from "./cockpit/Header.tsx";
import { Inbox } from "./cockpit/Inbox.tsx";
import { PlanView } from "./cockpit/PlanView.tsx";
import { Shell } from "./cockpit/Shell.tsx";
import { barStatus, StatusBar } from "./cockpit/StatusBar.tsx";
import { resetLayout } from "./cockpit/layout.ts";
import type { LayoutPanels } from "./cockpit/layout.ts";
import type { SettingsChanges } from "../protocol/cockpit.ts";
import { LegendDialog } from "./dialogs/LegendDialog.tsx";
import { LogsDialog } from "./dialogs/LogsDialog.tsx";
import { SettingsDialog } from "./dialogs/SettingsDialog.tsx";
import type { FileReaderBoundary } from "./composer/attachments.ts";

const MemoTree = memo(ContextTree);
const MemoPlan = memo(PlanView);
const MemoInbox = memo(Inbox);

export type { FileReaderBoundary };
export interface AppProps { transport: Transport; scheduler: Scheduler; reader: FileReaderBoundary; newId?: () => string }

export function App(props: AppProps): ReactElement {
  const { transport, scheduler, reader, newId } = props;
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
  const onSelectSession = useCallback((id: string) => connection.selectSession(id), [connection]);
  const onSelectPlan = useCallback((id: string) => connection.selectPlan(id), [connection]);
  const onAction = useCallback((a: Parameters<typeof connection.action>[0]) => { connection.action(a); }, [connection]);
  const panelsRef = useRef<LayoutPanels | null>(null);
  const [legendOpen, setLegendOpen] = useState(false);
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
    if (!live || !restored.current || selectedPlanId === null || selectedSessionId === null) return;
    saveSelection(window.sessionStorage, { planId: selectedPlanId, sessionKey: selectedSessionId });
  }, [live, selectedPlanId, selectedSessionId]);
  return (
    <>
    <BootstrapOverlay />
    <Shell
      panelsRef={panelsRef}
      header={
        <Header
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
          onLogs={onLogs}
          onCosts={onCosts}
          onSettings={onSettings}
        />
      }
      tree={
        !live ? null : <MemoTree
          tree={cockpit.tree}
          selectedSessionId={selectedSessionId}
          selectedPlanId={cockpit.selectedPlanId}
          onSelectSession={onSelectSession}
          onSelectPlan={onSelectPlan}
          costsOn={costsOn}
          costs={costs}
          onAction={onAction}
        />
      }
      plan={!live ? null : <MemoPlan plan={cockpit.plan} onAction={onAction} />}
      chat={
        session === null ? (
          <p data-empty className="grid h-full place-items-center text-muted-foreground">Select a session</p>
        ) : (
          <ChatPane view={session} connection={connection} reader={reader} />
        )
      }
      inbox={
        !live ? null : <MemoInbox
          inbox={cockpit.inbox}
          onAction={onAction}
          onSelectSession={onSelectSession}
        />
      }
    />
    <LegendDialog open={legendOpen} onClose={() => setLegendOpen(false)} />
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
