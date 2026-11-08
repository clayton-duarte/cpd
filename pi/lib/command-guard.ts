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

import { longReason, LIMIT } from "./askguard.ts";
import { BOUND_SECONDS, reasonText } from "./noblock.ts";

export { BOUND_SECONDS };

/** Where an example is taught to run, read statically from where the skill teaches it: `cpd-lead`'s SKILL.md is `lead`, `cpd-manager`'s is `manager`, the explore rules of `references/spec-files.md` are `explore`, and a Spec, a test-command, a pull-request acceptance example or an `ask_user_question` shape is the Worker's (or the authoring Lead's, whose asks the runtime bounds the same way). */
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
  /** The runtime refusal code (`timeout`, `spec_invalid`, `shape_violation`, `explore_workflow_forbidden`, `ask_length`). */
  code: string;
  /** The refusal reason, as the runtime guard shows it. */
  reason: string;
}

/** What the guard found: every example it collected and every one a runtime guard would refuse. */
export interface GuardResult {
  examples: Example[];
  violations: Violation[];
}

/** The Spec fields every Job's Spec carries (commands.md §5; `core/src/job/spec.rs` `FIELDS`). */
const SPEC_REQUIRED = ["goal", "context", "touches", "acceptance", "out_of_scope"];
/** The Spec fields any Job may add (`resources`). */
const SPEC_OPTIONAL = ["resources"];
/** The two more a `test` Job's Spec carries (`tests`, `test_files`). */
const SPEC_TEST = ["tests", "test_files"];
/** The four fields of an explore Job's Spec, and only those (`parse_explore`). */
const EXPLORE_REQUIRED = ["goal", "context", "acceptance", "out_of_scope"];

/** The refusal explore's bash timeout rule gives (`pi/lib/explore.ts` `bashVerdict`). */
const exploreTimeoutReason = (n: number): string =>
  `φ: explore timeout: every bash call carries a timeout above 0 and at most ${BOUND_SECONDS} s, except a lone cpd ask, which has no upper bound; got ${n}`;

/** Where one file teaches its examples to run: its role (a SKILL.md), explore (the Explore Jobs section of spec-files.md) or the Worker/PR acceptance (every other reference example). */
function contextOf(path: string, line: number, exploreFrom: number): Context {
  const base = path.split("/").pop() ?? path;
  if (base === "SKILL.md") {
    if (path.startsWith("cpd-manager")) return "manager";
    if (path.startsWith("cpd-lead")) return "lead";
    return "worker";
  }
  if (base === "spec-files.md" && exploreFrom > 0 && line >= exploreFrom) return "explore";
  return "worker";
}

/** The seconds of a `timeout <n>` literal as it is written (`30`, `1.5m`, `2h`), or undefined when it is not a literal one. */
function timeoutSeconds(word: string, unit: string): number {
  return Number(word) * ({ "": 1, s: 1, m: 60, h: 3600, d: 86400 }[unit] ?? 1);
}

/** Every `timeout <n>` of an example whose bound exceeds {@link BOUND_SECONDS}, as the runtime rule writes it.</br>Refused. */
function timeoutHits(text: string): string[] {
  const hits: string[] = [];
  for (const m of text.matchAll(/\btimeout\s+(-[^\s]+\s+)*(\d+(?:\.\d+)?)([smhd]?)\b/g)) {
    if (timeoutSeconds(m[2]!, m[3] ?? "") > BOUND_SECONDS) hits.push(`timeout ${m[2]}${m[3] ?? ""}`);
  }
  return hits;
}

/** The 30 s bound, the explore bound and the shape/kind rule for one command example (a fenced line or an inline span). */
function exampleViolations(ex: Example): Violation[] {
  const out: Violation[] = [];
  if (ex.context !== "worker") {
    for (const hit of timeoutHits(ex.text)) {
      const reason = ex.context === "explore" ? exploreTimeoutReason(Number(/\d+(?:\.\d+)?/.exec(hit)![0])) : reasonText(hit, ex.context);
      out.push({ ...ex, code: "timeout", reason });
    }
  }
  if (/\bcpd\s+job\s+(?:add|edit)\b/.test(ex.text) && /--kind\b/.test(ex.text)) {
    const kinds = (/--kind\s+([a-z][a-z|]*)/.exec(ex.text)?.[1] ?? "work").split("|");
    const explores = kinds.filter((k) => k === "scout" || k === "research");
    if (explores.length > 0) {
      if (/--workflow\b/.test(ex.text)) {
        const kind = explores[0]!;
        out.push({ ...ex, code: "explore_workflow_forbidden", reason: `φ: a ${kind} Job belongs to the Plan, not to a Workflow: drop --workflow.` });
      }
    } else if (/--plan\b/.test(ex.text)) {
      out.push({ ...ex, code: "shape_violation", reason: `φ: a shaping Plan allows only the explore kinds scout and research, not ${kinds.join(", ")}.` });
    }
  }
  return out;
}

/** The `spec_invalid` a fenced JSON example would meet: every field the schema knows, none of the required ones missing. Undefined when the block is not a Spec. */
function specViolation(body: string, ex: Example): Violation | undefined {
  let value: unknown;
  try {
    value = JSON.parse(body);
  } catch {
    return undefined;
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) return undefined;
  const keys = Object.keys(value);
  if (!keys.includes("goal")) return undefined; // not a Job Spec example
  const explore = !keys.includes("touches");
  const required = explore ? EXPLORE_REQUIRED : SPEC_REQUIRED;
  const allowed = new Set(explore ? EXPLORE_REQUIRED : [...SPEC_REQUIRED, ...SPEC_OPTIONAL, ...SPEC_TEST]);
  const bad = keys.find((k) => !allowed.has(k));
  const missing = required.find((k) => !keys.includes(k));
  if (bad !== undefined) return { ...ex, code: "spec_invalid", reason: `φ: the Spec is invalid at $.${bad}: unknown field.` };
  if (missing !== undefined) return { ...ex, code: "spec_invalid", reason: `φ: the Spec is invalid at $.${missing}: missing field.` };
  return undefined;
}

/** The characters an `ask_user_question` call carries, summed over every string literal as `pi/lib/askguard.ts` counts the text fields. */
function askCharacters(src: string): number {
  let total = 0;
  for (const m of src.matchAll(/"((?:\\.|[^"\\])*)"/g)) total += m[1]!.replace(/\\(.)/g, "$1").length;
  return total;
}

/** The `ask_length` an `ask_user_question` call shape would meet when its text is over {@link LIMIT}. */
function askViolation(body: string, ex: Example): Violation | undefined {
  const n = askCharacters(body);
  return n > LIMIT ? { ...ex, code: "ask_length", reason: longReason(n) } : undefined;
}

/** Collect the examples and judge them: every fenced line and inline span (the 30 s bound and the shape/kind rule), every fenced JSON Spec block (the Spec check) and every `ask_user_question` shape (the ask limit). */
function scan(files: SkillFile[]): GuardResult {
  const examples: Example[] = [];
  const violations: Violation[] = [];
  for (const file of files) {
    const lines = file.text.split("\n");
    const exploreFrom = /(^|\/)spec-files\.md$/.test(file.path) ? lines.findIndex((l) => /^##\s+Explore Jobs\b/.test(l)) + 1 : 0;
    const at = (line: number): Context => contextOf(file.path, line, exploreFrom);
    let fence: { info: string; line: number; body: string[] } | null = null;
    for (let i = 0; i < lines.length; i++) {
      const raw = lines[i]!;
      const line = i + 1;
      const marker = /^\s*```(\S*)\s*$/.exec(raw);
      if (marker) {
        if (fence) {
          const ex: Example = { file: file.path, line: fence.line, text: fence.body.join("\n").trim(), context: at(fence.line) };
          const body = fence.body.join("\n");
          if (/^json/.test(fence.info)) {
            const v = specViolation(body, ex);
            if (v) violations.push(v);
          } else if (/^ts/.test(fence.info) && body.includes("ask_user_question(")) {
            const v = askViolation(body, ex);
            if (v) violations.push(v);
          }
          fence = null;
        } else {
          fence = { info: (marker[1] ?? "").trim(), line, body: [] };
        }
        continue;
      }
      if (fence) {
        fence.body.push(raw);
        const text = raw.trim();
        if (text) {
          const ex: Example = { file: file.path, line, text, context: at(line) };
          examples.push(ex);
          violations.push(...exampleViolations(ex));
        }
      }
      for (const m of raw.matchAll(/`([^`\n]+)`/g)) {
        const text = m[1]!.trim();
        if (!text) continue;
        const ex: Example = { file: file.path, line, text, context: at(line) };
        examples.push(ex);
        violations.push(...exampleViolations(ex));
      }
    }
  }
  // A fenced line and its inline span can be the same example; report each refusal once.
  const seen = new Set<string>();
  const unique = violations.filter((v) => {
    const key = `${v.file}\n${v.line}\n${v.code}\n${v.reason}`;
    return seen.has(key) ? false : (seen.add(key), true);
  });
  return { examples, violations: unique };
}

/** Every command example of the files, in file order. */
export function collectExamples(files: SkillFile[]): Example[] {
  return scan(files).examples;
}

/** Every example a runtime guard would refuse (`timeout` over 30 s in Lead, Manager or explore context; an invalid Spec example; a kind its shape forbids; an over-1500-character ask). */
export function guardExamples(files: SkillFile[]): Violation[] {
  return scan(files).violations;
}

/** Collect the examples and judge them. */
export function guard(files: SkillFile[]): GuardResult {
  return scan(files);
}
