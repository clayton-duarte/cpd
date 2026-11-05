const key = (sessionId: string): string => `cpd-ui:draft:${sessionId}`;

export function loadDraft(sessionId: string): string {
  try {
    return window.localStorage.getItem(key(sessionId)) ?? "";
  } catch {
    return "";
  }
}

export function saveDraft(sessionId: string, text: string): void {
  try {
    if (text === "") window.localStorage.removeItem(key(sessionId));
    else window.localStorage.setItem(key(sessionId), text);
  } catch {
    // storage unavailable: keep in-memory state only
  }
}

export function clearDraft(sessionId: string): void {
  saveDraft(sessionId, "");
}
