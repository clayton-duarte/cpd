import type { SessionDot, Urgency } from "../protocol/cockpit.ts";

export type Tone = "blocked" | "landing" | "decision" | "active" | "focus" | "done" | "unread" | "neutral";
export type Glyph = "blocked" | "landing" | "decision" | "draft" | "active" | "focus" | "done" | "unread" | "queued" | "idle";
export interface Display { readonly tone: Tone; readonly glyph: Glyph; readonly label: string }
export type InboxTier = 1 | 2 | 3 | 4 | 5 | 6;
export interface InboxDisplay extends Display { readonly tier: InboxTier; readonly tierLabel: string }
export type SessionDisplay = "error" | "landing" | "dialog" | "working" | "unread" | "idle" | "asleep";

/** Stub (not implemented): the design doc 2d table. */
export function jobDisplay(_raw: string): Display {
  throw new Error("not implemented");
}
export function inboxDisplay(_urgency: Urgency): InboxDisplay {
  throw new Error("not implemented");
}
export function dotDisplay(_dot: SessionDot): SessionDisplay {
  throw new Error("not implemented");
}
export const knownInboxKinds: readonly string[] = [];
