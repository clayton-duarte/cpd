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
    const [context] = parts;
    if (!CONTEXT_RE.test(context)) {
      return null;
    }
    return {
      kind: "manager",
      context,
    };
  }
  if (parts.length === 2) {
    const [context, plan] = parts;
    if (!CONTEXT_RE.test(context) || !PLAN_RE.test(plan)) {
      return null;
    }
    return {
      kind: "lead",
      context,
      plan,
    };
  }
  if (parts.length === 3) {
    const [context, plan, job] = parts;
    if (!CONTEXT_RE.test(context) || !PLAN_RE.test(plan) || !JOB_RE.test(job)) {
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

export function isValidAgentKey(raw: string): boolean {
  return parseAgentKey(raw) !== null;
}

export function agentKeyToPath(raw: string): string | null {
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

export function pathToAgentKey(path: string): string | null {
  if (!path.startsWith("/a/")) return null;
  const parts = path.slice(3).split("/").filter(Boolean);
  if (parts.length === 1) {
    return isValidAgentKey(parts[0]) ? parts[0] : null;
  }
  if (parts.length === 2) {
    const key = `${parts[0]}:${parts[1]}`;
    return isValidAgentKey(key) ? key : null;
  }
  if (parts.length === 3) {
    const key = `${parts[0]}:${parts[1]}:${parts[2]}`;
    return isValidAgentKey(key) ? key : null;
  }
  return null;
}
