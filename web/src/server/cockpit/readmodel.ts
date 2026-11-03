import type { ContextTreeModel, InboxModel, PlanViewModel } from "../../protocol/cockpit.ts";
import type { CockpitPane } from "../../protocol/wire.ts";
import type { Cpd } from "./cli.ts";

type Reply = Readonly<Record<string, unknown>>;
export type RawContext = Readonly<Record<string, unknown>>;
export type RawPlan = Readonly<Record<string, unknown>>;
export type RawSession = Readonly<Record<string, unknown>>;
export type RawInboxItem = Readonly<Record<string, unknown>>;
export type RawPlanShow = Readonly<Record<string, unknown>>;
export interface ParsedInbox {
  rows: RawInboxItem[];
  engineRunning: boolean;
  hidden: { claimed: number; dismissed: number; snoozed: number };
  cursor: number;
}

export type PaneSet = ReadonlySet<CockpitPane>;
export interface PaneUpdate { tree?: ContextTreeModel; plan?: PlanViewModel | null; inbox?: InboxModel }
export interface ReadModel {
  loadAll(planId: string | null): Promise<{ tree: ContextTreeModel; plan: PlanViewModel | null; inbox: InboxModel }>;
  loadPanes(panes: PaneSet, planId: string | null): Promise<PaneUpdate>;
}
export function createReadModel(_cpd: Cpd): ReadModel {
  throw new Error("not implemented");
}
export function parseContextList(_reply: Reply): RawContext[] {
  throw new Error("not implemented");
}
export function parsePlanList(_reply: Reply): RawPlan[] {
  throw new Error("not implemented");
}
export function parseSessionList(_reply: Reply): RawSession[] {
  throw new Error("not implemented");
}
export function parseInboxList(_reply: Reply): ParsedInbox {
  throw new Error("not implemented");
}
export function parsePlanShow(_reply: Reply): RawPlanShow {
  throw new Error("not implemented");
}
export function buildTree(_input: { contexts: RawContext[]; plans: RawPlan[]; sessions: RawSession[]; inbox: RawInboxItem[]; cursor: number }): ContextTreeModel {
  throw new Error("not implemented");
}
export function buildPlanView(_raw: RawPlanShow): PlanViewModel {
  throw new Error("not implemented");
}
export function buildInbox(_parsed: ParsedInbox): InboxModel {
  throw new Error("not implemented");
}
