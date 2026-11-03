import type { ActionOutput, CockpitAction } from "../../protocol/cockpit.ts";
import type { Disposable, Scheduler } from "../../protocol/runtime.ts";
import type { CockpitPane, CockpitPaneData } from "../../protocol/wire.ts";
import type { ContextTreeModel, InboxModel, PlanViewModel } from "../../protocol/cockpit.ts";
import type { WireError } from "../../protocol/domain.ts";
import type { Cpd, FollowBoundary } from "./cli.ts";
import type { ReadModel } from "./readmodel.ts";

export interface CpdEvent { readonly id: number; readonly kind: string; readonly plan: string | null; readonly context: string | null; readonly session: string | null; readonly job: string | null; readonly attempt: string | null; readonly at: string; readonly text: string }
export function parseEventLine(_line: string): CpdEvent {
  throw new Error("not implemented");
}
export function panesFor(_event: CpdEvent, _selectedPlan: { id: string; contextId: string } | null): ReadonlySet<CockpitPane> {
  throw new Error("not implemented");
}

export const COALESCE_MS = 250;
export const SAFETY_REFRESH_MS = 30_000;
export const FOLLOW_RESTART_BASE_MS = 1_000;

export type CockpitUpdate = ({ kind: "pane" } & CockpitPaneData) | { kind: "error"; error: WireError };
export interface CockpitSnapshot { tree: ContextTreeModel | null; plan: PlanViewModel | null; inbox: InboxModel | null }

export interface CockpitService {
  start(): Promise<void>;
  stop(): void;
  snapshot(): CockpitSnapshot;
  selectedPlanId(): string | null;
  selectPlan(planId: string | null): Promise<void>;
  subscribe(listener: (u: CockpitUpdate) => void): Disposable;
  runAction(action: CockpitAction): Promise<ActionOutput>;
}
export function createCockpitService(_deps: { readModel: ReadModel; cpd: Cpd; follow: FollowBoundary; scheduler: Scheduler }): CockpitService {
  throw new Error("not implemented");
}
