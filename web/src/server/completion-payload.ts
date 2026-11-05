import type { SessionRef, TranscriptItem } from "../protocol/domain.ts";
import type { PlanViewModel } from "../protocol/cockpit.ts";
import type { CompletionPayload, PlanFacts } from "./completion-classify.ts";

const PAIRS = new Set(["context decide", "context note", "plan new", "plan lock", "plan phase", "workflow add", "job add", "session send"]);
const SINGLES = new Set(["ask", "answer"]);
const TEXT_MAX = 600;
const ACTION_MAX = 160;
const TRANSCRIPT_MAX = 40;

const commandOf = (item: TranscriptItem): string | null => {
  if (item.kind !== "tool") return null;
  const c = (item.args as Record<string, unknown> | null | undefined)?.["command"];
  return typeof c === "string" ? c : null;
};

export function ledgerActionsOf(items: readonly TranscriptItem[]): string[] {
  const out: string[] = [];
  for (const item of items) {
    const command = commandOf(item);
    if (command === null) continue;
    for (const segment of command.split(/&&|\|\||[;|\n]/)) {
      const s = segment.trim();
      const [bin, a, b] = s.split(/\s+/);
      if (bin !== "cpd" || a === undefined) continue;
      if (SINGLES.has(a) || (b !== undefined && PAIRS.has(`${a} ${b}`))) out.push(s.slice(0, ACTION_MAX));
    }
  }
  return out;
}

const cut = (s: string): string => s.slice(0, TEXT_MAX);

function entryOf(item: TranscriptItem): { kind: string; text: string } | null {
  switch (item.kind) {
    case "user":
      return { kind: "user", text: cut(item.text) };
    case "assistant":
      return { kind: "assistant", text: cut(item.blocks.flatMap((b) => (b.type === "text" ? [b.text] : [])).join("")) };
    case "tool": {
      const command = commandOf(item);
      return { kind: "tool", text: cut(`${item.name} ${command ?? JSON.stringify(item.args ?? null).slice(0, 120)}`) };
    }
    default:
      return null;
  }
}

function factsOf(plan: PlanViewModel): PlanFacts {
  return {
    id: plan.planId,
    title: plan.title,
    phase: plan.phase,
    counts: plan.counts,
    workflows: plan.workflows.map((w) => ({ key: w.key, title: w.title, status: w.status, prState: w.pr?.state ?? null })),
    jobs: plan.jobs.map((j) => ({ key: j.key, kind: j.kind, status: j.status, title: j.title })),
    hasPendingReview: plan.review !== null && plan.review.current,
    openEscalations: plan.openEscalations,
  };
}

export function buildPayload(input: { ref: SessionRef; hasOpenDialog: boolean; plan: PlanViewModel | null; items: readonly TranscriptItem[] }): CompletionPayload {
  const { ref } = input;
  const transcript = input.items.flatMap((i) => entryOf(i) ?? []).slice(-TRANSCRIPT_MAX);
  return {
    session: { id: ref.id, key: ref.key, role: "lead", state: ref.state, hasOpenDialog: input.hasOpenDialog },
    plan: input.plan === null ? null : factsOf(input.plan),
    ledgerActions: ledgerActionsOf(input.items),
    transcript,
  };
}
