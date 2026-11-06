export type FlowPhase = "context" | "plan" | "dispatch";
export type FlowAgent = "manager" | "lead" | "worker";
export interface FlowLoop {
  readonly to: string;
  readonly label: string;
  readonly condition: string;
}
export interface FlowStep {
  readonly id: string;
  readonly name: string;
  readonly phase: FlowPhase;
  readonly agent: FlowAgent;
  readonly artifact: string | null;
  readonly description: string;
  readonly loop: FlowLoop | null;
}

export const FLOW_STEPS: readonly FlowStep[] = [];

export function forwardEdgeIds(): readonly string[] {
  return [];
}

export function loopEdgeIds(): readonly string[] {
  return [];
}
