import type { Completion, SessionRef } from "../protocol/domain.ts";
import type { PlanViewModel } from "../protocol/cockpit.ts";
import type { SessionServerMessage } from "../protocol/wire.ts";
import type { CompletionClassifier } from "./completion-classify.ts";
import type { CompletionStore } from "./completion-store.ts";
import { buildPayload } from "./completion-payload.ts";
import { appendItems, applyDelta, emptyTranscript, prependEarlier, removeItems, setTail, updateItems } from "../state/transcript.ts";

export interface CompletionTracker { observe(m: SessionServerMessage): void; dispose(): void }

export function createCompletionService(deps: { classifier: CompletionClassifier; planOf: (ref: SessionRef) => Promise<PlanViewModel | null>; store?: CompletionStore }): { track(ref: SessionRef, send: (m: SessionServerMessage) => void): CompletionTracker } {
  const cache = new Map<string, Completion>();
  const last = new Map<string, Completion>();
  const inflight = new Map<string, Promise<Completion | null>>();
  const latest = new Map<string, number>();
  const writes = new Map<string, Promise<void>>();
  let nextRequest = 0;
  return {
    track(ref, send) {
      let transcript = emptyTranscript;
      let dialogs = new Set<string>();
      let busy = false;
      let disposed = false;
      let currentRequest = 0;
      const replay = ref.role === "lead" ? last.get(ref.id) : undefined;
      if (replay !== undefined) send({ type: "session.completion", sessionId: ref.id, completion: replay });

      const compute = async (request: number): Promise<void> => {
        const current = (): boolean => !disposed && currentRequest === request;
        if (ref.role !== "lead" || transcript.items.length === 0) return;
        const snapshot = transcript.items.slice();
        const hasOpenDialog = dialogs.size > 0;
        let plan: PlanViewModel | null;
        try {
          plan = await deps.planOf(ref);
        } catch {
          return;
        }
        if (plan === null || !current()) return;
        const entryId = String(snapshot[snapshot.length - 1]?.id);
        const key = `${ref.id}|${entryId}|${plan.cursor}`;
        let stored: Awaited<ReturnType<CompletionStore["get"]>> = null;
        try {
          stored = (await deps.store?.get(ref.id)) ?? null;
        } catch {
          stored = null;
        }
        if (!current()) return;
        let result: Completion | null;
        let persistResult = true;
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
              }
            });
          }
          const classified = await p;
          persistResult = classified !== null;
          result = classified ?? stored?.completion ?? null;
        }
        if (result === null || !current()) return;
        if (latest.get(ref.id) === request) {
          last.set(ref.id, result);
          const store = deps.store;
          if (store !== undefined && persistResult) {
            // Only persistence waits for earlier writes; classification and UI stay concurrent.
            const write = (writes.get(ref.id) ?? Promise.resolve()).then(async () => {
              if (current() && latest.get(ref.id) === request) await store.put(ref.id, entryId, result);
            }).catch(() => undefined);
            writes.set(ref.id, write);
            void write.then(() => { if (writes.get(ref.id) === write) writes.delete(ref.id); });
          }
        }
        send({ type: "session.completion", sessionId: ref.id, completion: result });
      };
      const schedule = (): void => {
        if (ref.role !== "lead") return;
        const request = ++nextRequest;
        currentRequest = request;
        latest.set(ref.id, request);
        void compute(request).catch(() => undefined);
      };

      return {
        observe(m) {
          if (disposed) return;
          try {
            switch (m.type) {
              case "session.window":
                if (m.mode === "tail") {
                  transcript = setTail(m.window, transcript);
                  schedule();
                } else transcript = prependEarlier(transcript, m.window);
                return;
              case "session.append":
                transcript = appendItems(transcript, m.items);
                return;
              case "session.update":
                transcript = updateItems(transcript, m.items);
                return;
              case "session.remove":
                transcript = removeItems(transcript, [m.itemId]);
                return;
              case "session.delta":
                transcript = applyDelta(transcript, m);
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
