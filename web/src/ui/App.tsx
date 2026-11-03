import { useEffect, useMemo } from "react";
import type { ReactElement } from "react";
import type { Scheduler, Transport } from "../protocol/runtime.ts";
import { createConnection } from "../state/connection.ts";
import { createStore, useStore } from "../state/store.ts";
import { ChatPane } from "./ChatPane.tsx";
import { ContextTree } from "./cockpit/ContextTree.tsx";
import { Inbox } from "./cockpit/Inbox.tsx";
import { PlanView } from "./cockpit/PlanView.tsx";
import { Shell } from "./cockpit/Shell.tsx";
import type { FileReaderBoundary } from "./composer/attachments.ts";

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
  const state = useStore(store, (s) => s);
  const { cockpit, session } = state;
  return (
    <Shell
      tree={
        <ContextTree
          tree={cockpit.tree}
          selectedSessionId={state.selectedSessionId}
          selectedPlanId={cockpit.selectedPlanId}
          onSelectSession={(id) => connection.selectSession(id)}
          onSelectPlan={(id) => connection.selectPlan(id)}
        />
      }
      plan={<PlanView plan={cockpit.plan} onAction={(a) => { connection.action(a); }} />}
      chat={
        session === null ? (
          <p data-empty>Select a session</p>
        ) : (
          <ChatPane view={session} connection={connection} reader={reader} />
        )
      }
      inbox={
        <Inbox
          inbox={cockpit.inbox}
          onAction={(a) => { connection.action(a); }}
          onSelectSession={(id) => connection.selectSession(id)}
        />
      }
    />
  );
}
