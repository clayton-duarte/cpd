/** commands.md §15.14 (hs-d544): magenta mentions of Leads and Managers. Stub: not implemented. */
export type PaneRole = "lead" | "manager" | "worker";

export function isMention(_token: string, _ids: ReadonlySet<string>): boolean {
  return false;
}

export function paintMentions(text: string, _ids: ReadonlySet<string>, _role: PaneRole = "lead"): string {
  return text;
}
