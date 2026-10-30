import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { CpdError, isObject } from "./cpd.ts";
import type { Host } from "./host.ts";
import type { Run } from "./run.ts";

// A Session is told its skill changed (commands.md §15.23).

export const SKILLS_TYPE = "cpd-skills";

export type Digest = Record<string, string>;
export type Change = { file: string; change: "added" | "removed" | "modified" };
/** The notice a Run holds until its next turn: the changes and the digest that becomes the baseline when it is told. */
export type SkillsNotice = { skill: string; dir: string; changes: Change[]; files: Digest };

/** The skill of a role (commands.md §15.23 "Who"); other roles have none. */
export function skillOf(role: string): string | undefined {
  return role === "lead" ? "cpd-lead" : role === "manager" ? "cpd-manager" : undefined;
}

/** sha256 of every regular file under `dir`, keyed by its `/` path relative to `dir`. */
export function digestSkill(dir: string): Digest {
  const files: Digest = {};
  const walk = (rel: string) => {
    for (const e of readdirSync(join(dir, rel), { withFileTypes: true })) {
      const path = rel === "" ? e.name : `${rel}/${e.name}`;
      if (e.isDirectory()) walk(path);
      else if (e.isFile()) files[path] = createHash("sha256").update(readFileSync(join(dir, path))).digest("hex");
    }
  };
  try {
    walk("");
  } catch (e) {
    throw new CpdError("skills_unreadable", `cannot read ${dir}: ${e instanceof Error ? e.message : String(e)}`);
  }
  if (!("SKILL.md" in files)) throw new CpdError("skills_unreadable", `${dir} holds no SKILL.md`);
  return files;
}

const isDigest = (v: unknown): v is Digest => isObject(v) && Object.values(v).every((h) => typeof h === "string" && /^[0-9a-f]{64}$/.test(h));

/** The last valid baseline of `skill` among a branch's entries, and how many `cpd-skills` entries do not parse. */
export function baselineFrom(entries: readonly unknown[], skill: string): { files: Digest | undefined; invalid: number } {
  let files: Digest | undefined;
  let invalid = 0;
  for (const e of entries) {
    if (!isObject(e) || e.type !== "custom" || e.customType !== SKILLS_TYPE) continue;
    const d = e.data;
    if (!isObject(d) || typeof d.skill !== "string" || !isDigest(d.files)) {
      invalid++;
      continue;
    }
    if (d.skill === skill) files = d.files;
  }
  return { files, invalid };
}

export function skillChanges(before: Digest, after: Digest): Change[] {
  const paths = [...new Set([...Object.keys(before), ...Object.keys(after)])].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  const out: Change[] = [];
  for (const file of paths) {
    const was = before[file];
    const now = after[file];
    if (was === now) continue;
    out.push({ file, change: was === undefined ? "added" : now === undefined ? "removed" : "modified" });
  }
  return out;
}

export function noticeContent(n: SkillsNotice): string {
  return `Skill ${n.skill} changed: ${n.changes.map((c) => `${c.file} ${c.change}`).join(", ")}. Re-read ${n.dir}/SKILL.md and each file named above before you rely on them.`;
}

/** Step 5b of §15.2: compares the directory with the baseline; an error is delivered and start-up goes on. */
export function checkSkills(host: Host, r: Run, entries: readonly unknown[], skill: string, dir: string): void {
  const { files: baseline, invalid } = baselineFrom(entries, skill);
  if (invalid > 0) host.deliver("session entries", new CpdError("entry_invalid", `${invalid} ${SKILLS_TYPE} entries of this session do not parse and were skipped`));
  let files: Digest;
  try {
    files = digestSkill(dir);
  } catch (e) {
    host.deliver("skills check", e);
    return;
  }
  if (baseline === undefined) {
    host.pi.appendEntry(SKILLS_TYPE, { skill, files });
    return;
  }
  const changes = skillChanges(baseline, files);
  if (changes.length === 0) return;
  r.skillsNotice = { skill, dir, changes, files };
  registerSkillsNotice(host);
}

/** Registered the first time a Run holds a notice; pi has no unsubscribe, so the Run is read at each call. */
function registerSkillsNotice(host: Host): void {
  if (host.registered.skills) return;
  host.registered.skills = true;
  host.pi.on("before_agent_start", () => {
    const r = host.run;
    const n = r?.skillsNotice;
    if (!r || !n) return undefined;
    r.skillsNotice = undefined;
    host.pi.appendEntry(SKILLS_TYPE, { skill: n.skill, files: n.files });
    return { message: { customType: SKILLS_TYPE, content: noticeContent(n), display: true, details: { skill: n.skill, dir: n.dir, changes: n.changes } } };
  });
}
