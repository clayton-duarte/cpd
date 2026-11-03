// STUB (red-test Job): the implementation Job replaces the bodies. Shared types are declared here only until their owning modules exist.
import type { SessionRef } from "../../protocol/domain.ts";
import type { ContextTreeModel, InboxModel, PlanViewModel } from "../../protocol/cockpit.ts";

export interface CockpitSnapshot { tree: ContextTreeModel | null; plan: PlanViewModel | null; inbox: InboxModel | null }
export interface ResolvedSession {
  readonly ref: SessionRef;
  readonly file?: string;
  readonly socketPath?: string;
  readonly tierOptions: readonly string[];
}
export interface Registry { resolve(sessionId: string): ResolvedSession }

export const SOCKET_PATH_MAX = 107;
export function socketPathOf(_cpdHome: string, _sessionId: string): string {
  throw new Error("not implemented");
}
export function createRegistry(_deps: { snapshot: () => CockpitSnapshot; cpdHome: string }): Registry {
  throw new Error("not implemented");
}
