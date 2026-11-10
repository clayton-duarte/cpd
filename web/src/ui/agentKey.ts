import type { AgentKey } from "../generated/types.ts";

export type { AgentKey };
export type AgentRole = "manager" | "lead" | "worker";

export interface ParsedAgentKey {
  readonly kind: AgentRole;
  readonly context: string;
  readonly plan?: string;
  readonly job?: string;
}

const CONTEXT_RE = /^[a-z][a-z0-9-]{1,39}$/;
const PLAN_RE = /^[a-z][a-z0-9-]{1,23}$/;
const JOB_RE = /^[a-z][a-z0-9-]{0,31}$/;
const MAX_KEY_LEN = 98;

export function parseAgentKey(raw: string): ParsedAgentKey | null {
  if (typeof raw !== "string" || raw.length === 0 || raw.length > MAX_KEY_LEN) {
    return null;
  }
  const parts = raw.split(":");
  if (parts.length === 1) {
    const context = parts[0];
    if (context === undefined || !CONTEXT_RE.test(context)) {
      return null;
    }
    return {
      kind: "manager",
      context,
    };
  }
  if (parts.length === 2) {
    const context = parts[0];
    const plan = parts[1];
    if (context === undefined || plan === undefined || !CONTEXT_RE.test(context) || !PLAN_RE.test(plan)) {
      return null;
    }
    return {
      kind: "lead",
      context,
      plan,
    };
  }
  if (parts.length === 3) {
    const context = parts[0];
    const plan = parts[1];
    const job = parts[2];
    if (context === undefined || plan === undefined || job === undefined || !CONTEXT_RE.test(context) || !PLAN_RE.test(plan) || !JOB_RE.test(job)) {
      return null;
    }
    return {
      kind: "worker",
      context,
      plan,
      job,
    };
  }
  return null;
}

export function isValidAgentKey(raw: string): raw is AgentKey {
  return parseAgentKey(raw) !== null;
}

export function agentKeyToPath(raw: AgentKey): string | null {
  const parsed = parseAgentKey(raw);
  if (!parsed) return null;
  if (parsed.kind === "manager") {
    return `/a/${parsed.context}`;
  }
  if (parsed.kind === "lead") {
    return `/a/${parsed.context}/${parsed.plan}`;
  }
  return `/a/${parsed.context}/${parsed.plan}/${parsed.job}`;
}

export function pathToAgentKey(path: string): AgentKey | null {
  if (!path.startsWith("/a/")) return null;
  const parts = path.slice(3).split("/").filter(Boolean);
  if (parts.length === 1) {
    const p0 = parts[0];
    return p0 !== undefined && isValidAgentKey(p0) ? p0 : null;
  }
  if (parts.length === 2) {
    const p0 = parts[0];
    const p1 = parts[1];
    if (p0 === undefined || p1 === undefined) return null;
    const key = `${p0}:${p1}`;
    return isValidAgentKey(key) ? key : null;
  }
  if (parts.length === 3) {
    const p0 = parts[0];
    const p1 = parts[1];
    const p2 = parts[2];
    if (p0 === undefined || p1 === undefined || p2 === undefined) return null;
    const key = `${p0}:${p1}:${p2}`;
    return isValidAgentKey(key) ? key : null;
  }
  return null;
}
