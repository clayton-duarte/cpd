import type { SessionDot, Urgency } from "../protocol/cockpit.ts";
import type { SessionState } from "../protocol/domain.ts";

export type Tone = "blocked" | "landing" | "decision" | "active" | "focus" | "done" | "unread" | "neutral";
export type Glyph = "blocked" | "landing" | "decision" | "draft" | "active" | "focus" | "done" | "unread" | "queued" | "idle";

export interface ToneClasses { readonly text: string; readonly textBright: string; readonly border: string; readonly tint: string }
export const toneClasses: Readonly<Record<Tone, ToneClasses>> = {
  blocked: { text: "text-status-blocked", textBright: "text-status-blocked-bright", border: "border-status-blocked", tint: "bg-status-blocked/10" },
  landing: { text: "text-status-landing", textBright: "text-status-landing-bright", border: "border-status-landing", tint: "bg-status-landing/10" },
  decision: { text: "text-status-decision", textBright: "text-status-decision-bright", border: "border-status-decision", tint: "bg-status-decision/10" },
  active: { text: "text-status-active", textBright: "text-status-active-bright", border: "border-status-active", tint: "bg-status-active/10" },
  focus: { text: "text-status-focus", textBright: "text-status-focus-bright", border: "border-status-focus", tint: "bg-status-focus/15" },
  done: { text: "text-status-done", textBright: "text-status-done-bright", border: "border-border", tint: "" },
  unread: { text: "text-foreground", textBright: "text-fg-strong", border: "border-border", tint: "" },
  neutral: { text: "text-muted-foreground", textBright: "text-fg-strong", border: "border-dashed border-line", tint: "" },
};

export const glyphClass: Readonly<Record<Glyph, string>> = {
  blocked: "glyph glyph-blocked",
  landing: "glyph glyph-landing",
  decision: "glyph glyph-decision",
  draft: "glyph glyph-draft",
  active: "glyph glyph-active",
  focus: "glyph glyph-focus",
  done: "glyph glyph-done",
  unread: "glyph glyph-unread",
  queued: "glyph glyph-queued",
  idle: "glyph glyph-idle",
};

export interface Display { readonly tone: Tone; readonly glyph: Glyph; readonly label: string }

/** The unknown case: cpd's status strings are open; a value outside the union carries its raw text. */
export type Resolved<T> =
  | { readonly kind: "known"; readonly value: T }
  | { readonly kind: "unknown"; readonly raw: string };
export const UNKNOWN_DISPLAY: { readonly tone: "neutral"; readonly glyph: "idle" } = { tone: "neutral", glyph: "idle" };

// jobs
export type JobStatus = "draft" | "queued" | "running" | "landing" | "checking" | "merging" | "merged" | "failed" | "cancelled";
export const jobStatus: Readonly<Record<JobStatus, Display>> = {
  draft: { tone: "decision", glyph: "draft", label: "draft" },
  queued: { tone: "neutral", glyph: "queued", label: "queued" },
  running: { tone: "active", glyph: "active", label: "running" },
  landing: { tone: "landing", glyph: "landing", label: "landing" },
  checking: { tone: "landing", glyph: "landing", label: "checking" },
  merging: { tone: "landing", glyph: "landing", label: "merging" },
  merged: { tone: "done", glyph: "done", label: "merged" },
  failed: { tone: "blocked", glyph: "blocked", label: "failed" },
  cancelled: { tone: "neutral", glyph: "idle", label: "cancelled" },
};
const jobStatuses: readonly JobStatus[] = ["draft", "queued", "running", "landing", "checking", "merging", "merged", "failed", "cancelled"];
export function parseJobStatus(raw: string): Resolved<JobStatus> {
  const value = jobStatuses.find((s) => s === raw);
  return value === undefined ? { kind: "unknown", raw } : { kind: "known", value };
}
export function jobDisplay(raw: string): Display {
  const p = parseJobStatus(raw);
  return p.kind === "known" ? jobStatus[p.value] : { ...UNKNOWN_DISPLAY, label: p.raw };
}

// sessions and plans
export type SessionDisplay = "error" | "landing" | "dialog" | "working" | "unread" | "idle" | "asleep";
export const sessionDisplay: Readonly<Record<SessionDisplay, Display>> = {
  error: { tone: "blocked", glyph: "blocked", label: "Needs attention" },
  landing: { tone: "landing", glyph: "landing", label: "Landing" },
  dialog: { tone: "decision", glyph: "decision", label: "Waiting on you" },
  working: { tone: "active", glyph: "active", label: "Working" },
  unread: { tone: "unread", glyph: "unread", label: "Unread reply" },
  idle: { tone: "neutral", glyph: "idle", label: "Idle" },
  asleep: { tone: "neutral", glyph: "idle", label: "Asleep" },
};
const attentionDisplay: Readonly<Record<Urgency, SessionDisplay>> = {
  red: "error",
  yellow: "landing",
  magenta: "dialog",
  blue: "dialog",
  cyan: "working",
  white: "unread",
};
const stateDisplay: Readonly<Record<SessionState, SessionDisplay>> = {
  working: "working",
  waiting: "idle",
  blocked: "error",
  idle: "idle",
  stopped: "asleep",
};
export function dotDisplay(dot: SessionDot): SessionDisplay {
  if (dot.state === "blocked") return "error";
  return dot.attention === null ? stateDisplay[dot.state] : attentionDisplay[dot.attention];
}
export const planPrecedence: readonly SessionDisplay[] = ["error", "landing", "dialog", "working", "unread", "idle", "asleep"];
export function planState(conditions: readonly SessionDisplay[]): SessionDisplay {
  return planPrecedence.find((p) => conditions.includes(p)) ?? "idle";
}

// inbox
export type InboxTier = 1 | 2 | 3 | 4 | 5 | 6;
export interface InboxDisplay extends Display { readonly tier: InboxTier; readonly tierLabel: string }
export const urgencyDisplay: Readonly<Record<Urgency, InboxDisplay>> = {
  red: { tier: 1, tierLabel: "blocking", tone: "blocked", glyph: "blocked", label: "blocking" },
  yellow: { tier: 2, tierLabel: "delivery", tone: "landing", glyph: "landing", label: "delivery" },
  blue: { tier: 3, tierLabel: "review", tone: "focus", glyph: "focus", label: "review" },
  magenta: { tier: 4, tierLabel: "decide", tone: "decision", glyph: "decision", label: "decide" },
  cyan: { tier: 5, tierLabel: "in flight", tone: "active", glyph: "active", label: "in flight" },
  white: { tier: 6, tierLabel: "unread", tone: "unread", glyph: "unread", label: "unread" },
};
export function inboxDisplay(urgency: Urgency): InboxDisplay {
  return urgencyDisplay[urgency];
}
export const knownInboxKinds: readonly string[] = [
  "engine-down", "loop-guard", "deploy", "pr-ready", "pr-closed", "review-requested",
  "dialog", "plan-review", "drafts-parked", "curate-retire", "unread",
];

// plan phases
export type Phase = "grounding" | "plan" | "dispatch" | "deploy" | "done";
export const phaseTone: Readonly<Record<Phase, Tone>> = {
  grounding: "active", plan: "decision", dispatch: "active", deploy: "landing", done: "done",
};
export const phaseLabel: Readonly<Record<Phase, string>> = {
  grounding: "grounding", plan: "planning", dispatch: "dispatch", deploy: "deploy", done: "done",
};
const phases: readonly Phase[] = ["grounding", "plan", "dispatch", "deploy", "done"];
export function parsePhase(raw: string): Resolved<Phase> {
  const value = phases.find((p) => p === raw);
  return value === undefined ? { kind: "unknown", raw } : { kind: "known", value };
}
