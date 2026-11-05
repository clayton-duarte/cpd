import type { Completion, SessionRef, TranscriptItem } from "../protocol/domain.ts";
import type { PlanViewModel } from "../protocol/cockpit.ts";
import type { SessionServerMessage } from "../protocol/wire.ts";
import type { CompletionClassifier } from "./completion-classify.ts";
import { buildPayload } from "./completion-payload.ts";

export interface CompletionTracker { observe(m: SessionServerMessage): void; dispose(): void }

export function createCompletionService(deps: { classifier: CompletionClassifier; planOf: (ref: SessionRef) => Promise<PlanViewModel | null> }): { track(ref: SessionRef, send: (m: SessionServerMessage) => void): CompletionTracker } {
  const cache = new Map<string, Completion>();
  const inflight = new Map<string, Promise<Completion | null>>();
  return {
    track(ref, send) {
      let items: TranscriptItem[] = [];
      let dialogs = new Set<string>();
      let busy = false;
      let disposed = false;

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
        const key = `${ref.id}|${snapshot[snapshot.length - 1]?.id}|${plan.cursor}`;
        let result: Completion | null;
        const hit = cache.get(key);
        if (hit !== undefined) result = hit;
        else {
          let p = inflight.get(key);
          if (p === undefined) {
            p = deps.classifier.classify(buildPayload({ ref: { ...ref, role: "lead" }, hasOpenDialog, plan, items: snapshot })).catch(() => null);
            inflight.set(key, p);
            void p.then((c) => {
              inflight.delete(key);
              if (c !== null) cache.set(key, c);
            });
          }
          result = await p;
        }
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
