import type { Completion, SessionRef, TranscriptItem } from "../protocol/domain.ts";
import type { PlanViewModel } from "../protocol/cockpit.ts";
import type { SessionServerMessage } from "../protocol/wire.ts";
import type { CompletionClassifier } from "./completion-classify.ts";
import type { CompletionStore } from "./completion-store.ts";
import { buildPayload } from "./completion-payload.ts";

export interface CompletionTracker { observe(m: SessionServerMessage): void; dispose(): void }

export function createCompletionService(deps: { classifier: CompletionClassifier; planOf: (ref: SessionRef) => Promise<PlanViewModel | null>; store?: CompletionStore }): { track(ref: SessionRef, send: (m: SessionServerMessage) => void): CompletionTracker } {
  const cache = new Map<string, Completion>();
  const last = new Map<string, Completion>();
  const inflight = new Map<string, Promise<Completion | null>>();
  return {
    track(ref, send) {
      let items: TranscriptItem[] = [];
      let dialogs = new Set<string>();
      let busy = false;
      let disposed = false;
      const replay = ref.role === "lead" ? last.get(ref.id) : undefined;
      if (replay !== undefined) send({ type: "session.completion", sessionId: ref.id, completion: replay });

      const compute = async (): Promise<void> => {
        if (ref.role !== "lead" || items.length === 0) return;
        const snapshot = items.slice();
        const hasOpenDialog = dialogs.size > 0;
        let plan: PlanViewModel | null;
        try {
          plan = await deps.planOf(ref);
        } catch {
          return;
        }
        if (plan === null || disposed) return;
        const entryId = String(snapshot[snapshot.length - 1]?.id);
        const key = `${ref.id}|${entryId}|${plan.cursor}`;
        let stored: Awaited<ReturnType<CompletionStore["get"]>> = null;
        try {
          stored = (await deps.store?.get(ref.id)) ?? null;
        } catch {
          stored = null;
        }
        let result: Completion | null;
        const hit = cache.get(key);
        if (stored !== null && stored.entryId === entryId) result = stored.completion;
        else if (hit !== undefined) result = hit;
        else {
          let p = inflight.get(key);
          if (p === undefined) {
            p = deps.classifier.classify(buildPayload({ ref: { ...ref, role: "lead" }, hasOpenDialog, plan, items: snapshot })).catch(() => null);
            inflight.set(key, p);
            void p.then((c) => {
              inflight.delete(key);
              if (c !== null) {
                cache.set(key, c);
                void deps.store?.put(ref.id, entryId, c).catch(() => undefined);
              }
            });
          }
          result = (await p) ?? stored?.completion ?? null;
        }
        if (result === null) return;
        last.set(ref.id, result);
        if (!disposed) send({ type: "session.completion", sessionId: ref.id, completion: result });
      };
      const schedule = (): void => { void compute().catch(() => undefined); };

      return {
        observe(m) {
          if (disposed) return;
          try {
            switch (m.type) {
              case "session.window":
                if (m.mode === "tail") {
                  items = [...m.window.items];
                  schedule();
                } else items = [...m.window.items, ...items];
                return;
              case "session.append":
                items = [...items, ...m.items];
                return;
              case "session.update":
                items = items.map((it) => m.items.find((u) => u.id === it.id) ?? it);
                return;
              case "session.dialog":
                dialogs = new Set(dialogs).add(m.dialog.id);
                return;
              case "session.dialogResolved":
                dialogs = new Set([...dialogs].filter((d) => d !== m.dialogId));
                return;
              case "session.status": {
                const settled = busy && !m.busy;
                busy = m.busy;
                if (settled) schedule();
                return;
              }
              default:
                return;
            }
          } catch {
            // observe never throws
          }
        },
        dispose() { disposed = true; },
      };
    },
  };
}
