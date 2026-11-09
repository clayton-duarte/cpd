import type { Attachment } from "../../protocol/domain.ts";

const key = (sessionId: string): string => `cpd-ui:draft:${sessionId}`;
// A text-only draft is stored as the bare text; one with attachments is this prefix plus JSON.
// The prefix starts with a NUL, which a typed message cannot contain, so the two never collide.
const PREFIX = "\u0000cpd-draft:";

export interface Draft {
  readonly text: string;
  readonly attachments: readonly Attachment[];
}

export function loadDraft(sessionId: string): Draft {
  let raw: string | null;
  try {
    raw = window.localStorage.getItem(key(sessionId));
  } catch {
    return { text: "", attachments: [] };
  }
  if (raw === null) return { text: "", attachments: [] };
  if (!raw.startsWith(PREFIX)) return { text: raw, attachments: [] };
  try {
    const parsed = JSON.parse(raw.slice(PREFIX.length)) as Draft;
    if (typeof parsed.text !== "string" || !Array.isArray(parsed.attachments)) throw new Error("bad draft");
    return parsed;
  } catch {
    return { text: "", attachments: [] };
  }
}

/** Returns false when the draft could not be stored (quota, storage unavailable). */
export function saveDraft(sessionId: string, text: string, attachments: readonly Attachment[] = []): boolean {
  try {
    if (text === "" && attachments.length === 0) window.localStorage.removeItem(key(sessionId));
    else window.localStorage.setItem(key(sessionId), attachments.length === 0 ? text : PREFIX + JSON.stringify({ text, attachments }));
    return true;
  } catch {
    return false;
  }
}

export function clearDraft(sessionId: string): void {
  saveDraft(sessionId, "");
}
