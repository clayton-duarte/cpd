import { formatSkillsForPrompt, loadSkillsFromDir } from "@earendil-works/pi-coding-agent";
import { isObject } from "../lib/cpd.ts";
import { digestSkill, noticeContent, skillChanges, SKILLS_TYPE, type Change, type Digest, type SkillsNotice } from "../lib/skills.ts";

// Contract §6.18 "Skills": the role skill section and the digest check against the conversation's
// entries.

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

/** A `data.files` that is an object of 64-hex strings, as pi/lib/skills.ts's `isDigest` reads. */
const isDigest = (v: unknown): v is Digest => isObject(v) && Object.values(v).every((h) => typeof h === "string" && /^[0-9a-f]{64}$/.test(h));

/** The role's skills section: the name, description and path of its `SKILL.md` (contract §6.18, G35). */
export function skillsSection(skill: string, dir: string): string | undefined {
  const { skills } = loadSkillsFromDir({ dir, source: skill });
  const text = formatSkillsForPrompt(skills);
  return text === "" ? undefined : text;
}

/** The last valid baseline of `skill` among the host's entries, and how many `cpd-skills` entries do not parse (G34). */
function baselineFrom(entries: readonly { kind: string; data?: Record<string, unknown> }[], skill: string): { files: Digest | undefined; invalid: number } {
  let files: Digest | undefined;
  let invalid = 0;
  for (const e of entries) {
    if (e.kind !== SKILLS_TYPE) continue;
    const d = e.data;
    if (!isObject(d) || typeof d.skill !== "string" || !isDigest(d.files)) {
      invalid++;
      continue;
    }
    if (d.skill === skill) files = d.files;
  }
  return { files, invalid };
}

/** The §15.23 check against the conversation's entries: unreadable, no baseline, equal, or changed. */
export async function skillsCheck(port: SkillsPort, skill: string, dir: string): Promise<void> {
  const { files: baseline, invalid } = baselineFrom(await port.entries(), skill);
  if (invalid > 0) {
    port.deliver({ what: "session entries", code: "entry_invalid", message: `${invalid} ${SKILLS_TYPE} entries of this session do not parse and were skipped` });
  }
  let files: Digest;
  try {
    files = digestSkill(dir);
  } catch (e) {
    const code = e instanceof Error && "code" in e && typeof e.code === "string" ? e.code : "skills_unreadable";
    port.deliver({ what: "skills check", code, message: e instanceof Error ? e.message : String(e) });
    return;
  }
  if (baseline === undefined) {
    await port.write({ kind: SKILLS_TYPE, data: { skill, files }, model: [] });
    return;
  }
  const changes = skillChanges(baseline, files);
  if (changes.length === 0) return;
  const notice: SkillsNotice = { skill, dir, changes, files };
  // The new baseline is written first; then the wire notice and the model entry (contract §6.18 "the delivery").
  await port.write({ kind: SKILLS_TYPE, data: { skill, files }, model: [noticeContent(notice)] });
  port.broadcast({ type: "notice", level: "info", message: "skills changed" });
}
