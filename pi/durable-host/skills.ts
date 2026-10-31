import type { Change, Digest } from "../lib/skills.ts";

// Contract §6.18 "Skills": the role skill section and the digest check against the conversation's
// entries. This is a stub: the test Job types the exports, the implementation Job fills the bodies.

/** The effect port of the skills check (contract §6.18). Every member is required. */
export type SkillsPort = {
  entries(): Promise<readonly { kind: string; data?: Record<string, unknown> }[]>;
  write(entry: { kind: string; data: Record<string, unknown>; model: readonly unknown[] }): Promise<unknown>;
  broadcast(msg: { type: "notice"; level: "info" | "warning" | "error"; message: string }): void;
  deliver(error: { what: string; code: string; message: string; details?: Record<string, unknown> }): void;
  log(line: string): void;
};

/** The held notice a changed digest must deliver at the next turn (pi/lib/skills.ts `SkillsNotice`). */
export type SkillsNotice = { skill: string; dir: string; changes: Change[]; files: Digest };

/** The role's skills section: the name, description and path of its `SKILL.md` (contract §6.18, G35). */
export function skillsSection(skill: string, dir: string): string | undefined {
  throw new Error("not implemented");
}

/** The §15.23 check against the conversation's entries: unreadable, no baseline, equal, or changed. */
export async function skillsCheck(port: SkillsPort, skill: string, dir: string): Promise<void> {
  throw new Error("not implemented");
}
