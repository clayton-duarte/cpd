export type AgentRole = "manager" | "lead" | "worker";

export interface ParsedAgentKey {
  readonly kind: AgentRole;
  readonly context: string;
  readonly plan?: string;
  readonly job?: string;
}

export function parseAgentKey(raw: string): ParsedAgentKey | null {
  throw new Error("stub: parseAgentKey not implemented");
}

export function isValidAgentKey(raw: string): boolean {
  throw new Error("stub: isValidAgentKey not implemented");
}

export function agentKeyToPath(raw: string): string | null {
  throw new Error("stub: agentKeyToPath not implemented");
}

export function pathToAgentKey(path: string): string | null {
  throw new Error("stub: pathToAgentKey not implemented");
}
