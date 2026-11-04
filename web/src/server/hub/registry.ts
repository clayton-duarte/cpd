import { WebError, type SessionRef } from "../../protocol/domain.ts";
import type { ContextTreeModel, InboxModel, PlanViewModel } from "../../protocol/cockpit.ts";

export interface CockpitSnapshot { tree: ContextTreeModel | null; plan: PlanViewModel | null; inbox: InboxModel | null }
export interface ResolvedSession {
  readonly ref: SessionRef;
  readonly file?: string;
  readonly socketPath?: string;
  readonly tierOptions: readonly string[];
}
export interface Registry { resolve(sessionId: string): ResolvedSession }

export const SOCKET_PATH_MAX = 107; // config.ts:31 (scout-durable §1)
export function socketPathOf(cpdHome: string, sessionId: string): string {
  return `${cpdHome}/run/host/${sessionId}.sock`;
}

export function createRegistry(deps: { snapshot: () => CockpitSnapshot; cpdHome: string }): Registry {
  return {
    resolve(sessionId) {
      const tree = deps.snapshot().tree;
      const refs: SessionRef[] = [];
      for (const context of tree?.contexts ?? []) {
        for (const node of context.sessions) refs.push(node.ref);
        for (const plan of context.plans) for (const node of plan.sessions) refs.push(node.ref);
      }
      const ref = refs.find((r) => r.id === sessionId);
      if (ref === undefined) throw new WebError("session_not_found", `no session ${sessionId}`, { sessionId });
      const tierOptions = [...new Set([...refs.map((r) => r.tier), ref.tier].filter((t): t is string => t !== undefined))].sort();
      if (ref.runner === "pi") {
        if (ref.file === undefined) throw new WebError("session_file_missing", `session ${sessionId} has no transcript file`, { sessionId });
        return { ref, file: ref.file, tierOptions };
      }
      const socketPath = socketPathOf(deps.cpdHome, sessionId);
      if (Buffer.byteLength(socketPath) > SOCKET_PATH_MAX) {
        throw new WebError("host_unreachable", "socket path too long", { path: socketPath, reason: "socket_path_too_long" });
      }
      return { ref, socketPath, tierOptions };
    },
  };
}
