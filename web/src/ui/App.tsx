import { memo, useCallback, useEffect, useMemo } from "react";
import type { ReactElement } from "react";
import type { Scheduler, Transport } from "../protocol/runtime.ts";
import { createConnection } from "../state/connection.ts";
import { createStore, useStore } from "../state/store.ts";
import { ChatPane } from "./ChatPane.tsx";
import { ContextTree } from "./cockpit/ContextTree.tsx";
import { Header } from "./cockpit/Header.tsx";
import { Inbox } from "./cockpit/Inbox.tsx";
import { PlanView } from "./cockpit/PlanView.tsx";
import { Shell } from "./cockpit/Shell.tsx";
import { StatusBar } from "./cockpit/StatusBar.tsx";
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
  const live = cockpit.tree !== null; // panes mount on the first snapshot, not before
  return (
    <Shell
      header={<Header />}
      status={<StatusBar runner={session?.ref?.runner ?? null} state={session?.ref?.state ?? null} version={null} />}
      tree={
        !live ? null : <MemoTree
          tree={cockpit.tree}
          selectedSessionId={selectedSessionId}
          selectedPlanId={cockpit.selectedPlanId}
          onSelectSession={onSelectSession}
          onSelectPlan={onSelectPlan}
        />
      }
      plan={!live ? null : <MemoPlan plan={cockpit.plan} onAction={onAction} />}
      chat={
        session === null ? (
          <p data-empty>Select a session</p>
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
  );
}
