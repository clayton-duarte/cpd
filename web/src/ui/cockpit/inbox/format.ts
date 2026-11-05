import type { Urgency } from "../../../protocol/cockpit.ts";

<<<<<<< ours
=======
/** TUI urgency glyphs (legend.rs). */
const urgencyGlyph: Readonly<Record<Urgency, string>> = {
  red: "\u{F0362}",
  yellow: "\u{F069A}",
  magenta: "\u{F173A}",
  blue: "\u{F0368}",
  cyan: "\u{F06F1}",
  white: "\u{F0366}",
};
/** Kinds with a glyph of their own, over the urgency's. */
const kindGlyph: Readonly<Record<string, string>> = {
  deploy: "\u{F15A9}",
  "pr-ready": "\u{F12F4}",
  "review-requested": "\u{F12F2}",
  "curate-retire": "\u{F116E}",
};
export const LINK_CUE = "\u{F11AD}";
>>>>>>> theirs
export const linkKinds: readonly string[] = ["plan-review", "pr-ready", "pr-closed", "review-requested", "deploy"];
/** Kinds whose left click opens the row's session (a Lead for the Plan kinds: the read model sets sessionId). */
export const sessionKinds: readonly string[] = ["dialog", "unread", "loop-guard", "question", "plan-review", "drafts-parked", "pr-ready", "pr-closed"];

export const urgencyText: Readonly<Record<Urgency, string>> = {
  red: "text-status-blocked",
  yellow: "text-status-landing",
  blue: "text-status-focus",
  magenta: "text-status-decision",
  cyan: "text-status-active",
  white: "text-muted-foreground",
};

export function glyphOf(kind: string, urgency: Urgency): string {
  return kindGlyph[kind] ?? urgencyGlyph[urgency];
}

export function age(s: number): string {
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}
