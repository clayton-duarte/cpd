/**
 * The command-example guard (contract.md §8.3 "Delivery to agents"; docs/implementing.md
 * "The command-example guard", Plan `agent-refusals`, Workflow `enforce`): every command
 * example the skills and their Spec references teach must be one the Session that copies it
 * could actually run, because a skill that teaches a command the runtime guards refuse costs
 * its reader a turn for nothing.
 *
 * Pure: the caller reads the files and hands over their text; the guard collects the examples
 * and judges them statically. It runs nothing, reads no store and starts no process.
 */

/** The longest a Lead's, a Manager's or an explore Worker's shell call may wait, in seconds (`pi/lib/noblock.ts` `BOUND_SECONDS`; noblock G24, commands.md §15.11). */
export const BOUND_SECONDS = 30;

/** Where an example is taught to run, read statically from where the skill teaches it: `cpd-lead`'s SKILL.md is `lead`, `cpd-manager`'s is `manager`, a `references/spec-files.md` explore rule is `explore`, a fenced Spec or a pull-request acceptance example is `worker`. */
export type Context = "lead" | "manager" | "explore" | "worker";

/** One text the guard scans: its repo-relative path under `pi/skills/` (for example `cpd-lead/SKILL.md`) and its text. */
export interface SkillFile {
  path: string;
  text: string;
}

/** One command example the guard collected from the files. */
export interface Example {
  /** The file the example is written in, repo-relative. */
  file: string;
  /** The 1-based line the example starts on. */
  line: number;
  /** The example itself. */
  text: string;
  /** Where the skill teaches it to run. */
  context: Context;
}

/** One example a runtime guard would refuse, with the refusal code and its reason. */
export interface Violation extends Example {
  /** The runtime refusal code (`timeout`, `spec_invalid`, `shape_violation`, `explore_workflow_forbidden`). */
  code: string;
  /** The refusal reason, as the runtime guard shows it. */
  reason: string;
}

/** What the guard found: every example it collected and every one a runtime guard would refuse. */
export interface GuardResult {
  examples: Example[];
  violations: Violation[];
}

/** Every command example of the files, in file order. STUB: collects nothing. */
export function collectExamples(_files: SkillFile[]): Example[] {
  return [];
}

/** Every example a runtime guard would refuse (`timeout` over 30 s in Lead, Manager or explore context; an invalid Spec example; a kind its shape forbids; an over-1500-character ask). STUB: reports no violations. */
export function guardExamples(_files: SkillFile[]): Violation[] {
  return [];
}

/** Collect the examples and judge them. STUB: collects nothing and reports no violations, so the enforcement test is red by assertion. */
export function guard(_files: SkillFile[]): GuardResult {
  return { examples: [], violations: [] };
}
