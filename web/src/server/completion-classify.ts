import type { Completion, CompletionLabel, SessionState } from "../protocol/domain.ts";
import type { Scheduler } from "../protocol/runtime.ts";
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

export function buildClassifierRequest(_p: CompletionPayload): { state: CompletionPayload; question: string; labels: { name: string; definition: string }[] } {
  throw new Error("not implemented");
}

export function mapAnswer(_answer: unknown): Completion | null {
  return null;
}

export interface CompletionClassifier { classify(p: CompletionPayload): Promise<Completion | null> }

export function createCompletionClassifier(_deps: { child: ChildBoundary; scheduler: Scheduler; script: string | null; agentDir: string; env: NodeJS.ProcessEnv }): CompletionClassifier {
  return { classify: () => Promise.resolve(null) };
}
