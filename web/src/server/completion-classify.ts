import type { Completion, CompletionScore, CompletionLabel, CompletionTone, SessionState } from "../protocol/domain.ts";
import type { Disposable as SchedulerHandle, Scheduler } from "../protocol/runtime.ts";
import type { ChildBoundary } from "./plugin.ts";

export const COMPLETION_QUESTION =
  "Evaluate whether this Lead session is completely finished and safe to archive without losing any work, plan, decision, clarification, or escalation. Check all four criteria: (1) every user question, clarification, or ask was answered and no question awaits a reply; (2) all requested work is completed/merged (no unfinished or failed workflows/jobs left behind); (3) every decision agreed in conversation was registered; (4) every bug report or feature request raised in conversation was escalated or tracked. Pick safe_to_archive only when all four hold; otherwise pick the primary blocker.";

export const COMPLETION_LABELS: readonly { name: CompletionLabel; definition: string }[] = [
  { name: "safe_to_archive", definition: "All conversations and asks were answered, all work and workflows are completed, all decisions are registered, and all bugs or feature requests were escalated." },
  { name: "unanswered_ask", definition: "A user question, clarification request, or assistant ask is still unanswered or awaiting a reply." },
  { name: "open_work", definition: "Requested work, workflows, pull requests, or jobs are still in progress, unmerged, failed, or unstarted." },
  { name: "unrecorded_decision", definition: "A decision or design choice agreed in the conversation has not been registered in context or plan state." },
  { name: "unescalated_issue", definition: "A bug report or feature request discussed in the conversation has not been escalated or registered." },
];

export interface PlanFacts {
  id: string;
  title: string;
  phase: string;
  counts: unknown;
  workflows: { key: string; title: string; status: string; prState: string | null }[];
  jobs: { key: string; kind: string; status: string; title: string }[];
  hasPendingReview: boolean;
  openEscalations: number;
}

export interface CompletionPayload {
  session: { id: string; key: string; role: "lead"; state: SessionState; hasOpenDialog: boolean };
  plan: PlanFacts | null;
  ledgerActions: string[];
  transcript: { kind: string; text: string }[];
}

export function buildClassifierRequest(p: CompletionPayload): { state: CompletionPayload; question: string; labels: { name: string; definition: string }[] } {
  return { state: p, question: COMPLETION_QUESTION, labels: COMPLETION_LABELS.map((l) => ({ name: l.name, definition: l.definition })) };
}

const LABEL_TEXT: Record<CompletionLabel, string> = {
  safe_to_archive: "Safe to archive",
  unanswered_ask: "Unanswered ask",
  open_work: "Open work",
  unrecorded_decision: "Unrecorded decision",
  unescalated_issue: "Unescalated bug",
};

const isLabel = (v: unknown): v is CompletionLabel => typeof v === "string" && Object.hasOwn(LABEL_TEXT, v);
const num = (v: unknown): number | undefined => (typeof v === "number" && Number.isFinite(v) ? v : undefined);

export function mapAnswer(answer: unknown): Completion | null {
  if (typeof answer !== "object" || answer === null) return null;
  const a = answer as Record<string, unknown>;
  if (a["ok"] !== true || !isLabel(a["label"])) return null;
  const label = a["label"];
  const probs = typeof a["probabilities"] === "object" && a["probabilities"] !== null ? (a["probabilities"] as Record<string, unknown>) : {};
  const p = num(probs["safe_to_archive"]);
  const c = num(a["confidence"]);
  const x = label === "safe_to_archive" ? (p ?? c ?? 1) : (p ?? 1 - (c ?? 0.5));
  const score = Math.min(100, Math.max(0, Math.round(x * 100)));
  const tone: CompletionTone =
    label === "safe_to_archive" ? (score >= 80 ? "done" : "decision") : label === "open_work" ? (score >= 40 ? "decision" : "blocked") : "decision";
  const others: CompletionScore[] = [];
  for (const l of COMPLETION_LABELS) {
    if (l.name === label) continue;
    const q = num(probs[l.name]);
    if (q !== undefined) others.push({ label: l.name, score: Math.min(100, Math.max(0, Math.round(q * 100))) });
  }
  others.sort((x, y) => y.score - x.score);
  const base = { label, text: `${LABEL_TEXT[label]} · ${score}%`, score, tone };
  return others.length > 0 ? { ...base, others } : base;
}

export interface CompletionClassifier { classify(p: CompletionPayload): Promise<Completion | null> }

const DEADLINE_MS = 30_000;

export function createCompletionClassifier(deps: { child: ChildBoundary; scheduler: Scheduler; script: string | null; agentDir: string; env: NodeJS.ProcessEnv }): CompletionClassifier {
  return {
    classify: (p) =>
      new Promise<Completion | null>((resolve) => {
        const script = deps.script;
        if (script === null) return resolve(null);
        let timer: SchedulerHandle | undefined;
        let done = false;
        const finish = (v: Completion | null) => {
          if (done) return;
          done = true;
          timer?.dispose();
          resolve(v);
        };
        try {
          const child = deps.child.spawn("node", [script], { stdio: ["pipe", "pipe", "pipe"], env: { ...deps.env, PI_CODING_AGENT_DIR: deps.agentDir } });
          let out = "";
          child.stdout.on("data", (c) => { out += c.toString(); });
          child.stderr.on("data", () => undefined);
          child.stdin?.on("error", () => finish(null));
          child.on("close", (code) => {
            if (code !== 0) return finish(null);
            const last = out.split("\n").map((l) => l.trim()).filter((l) => l !== "").pop();
            if (last === undefined) return finish(null);
            try {
              finish(mapAnswer(JSON.parse(last)));
            } catch {
              finish(null);
            }
          });
          timer = deps.scheduler.after(DEADLINE_MS, () => {
            try { child.kill(); } catch { /* already gone */ }
            finish(null);
          });
          child.stdin?.write(JSON.stringify(buildClassifierRequest(p)));
          child.stdin?.end();
        } catch {
          finish(null);
        }
      }),
  };
}
