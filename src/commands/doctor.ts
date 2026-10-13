import { parseArgs } from "node:util";
import { loadConfig } from "../lib/config.ts";
import { CpdError, usage } from "../lib/errors.ts";
import { run as exec } from "../lib/exec.ts";
import type { Result } from "../lib/output.ts";
import { LETTERS, loadCatalog } from "../lib/tiers.ts";

type Check = { name: string; ok: boolean; found?: string; expected?: string; message?: string };

type Semver = [number, number, number];
const parseSemver = (s: string): Semver | null => {
  const m = /(\d+)\.(\d+)\.(\d+)/.exec(s);
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
};
const cmp = (a: Semver, b: Semver) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2];

type Tool = { name: string; args: string[]; expected: string; accepts: (v: Semver) => boolean };

const exactly = (want: string) => {
  const w = parseSemver(want)!;
  return (v: Semver) => cmp(v, w) === 0;
};

/** Contract §11 versions. pi is accepted at the contract version or any later 1.x (see report: the machine has 1.2.0). */
const TOOLS: Tool[] = [
  { name: "bd", args: ["--version"], expected: "1.3.1", accepts: exactly("1.3.1") },
  { name: "wt", args: ["--version"], expected: "0.80.0", accepts: exactly("0.80.0") },
  { name: "herdr", args: ["--version"], expected: "0.9.3", accepts: exactly("0.9.3") },
  { name: "pi", args: ["--version"], expected: ">=1.1.0 <2.0.0", accepts: (v) => v[0] === 1 && cmp(v, [1, 1, 0]) >= 0 },
];

async function toolCheck(tool: Tool): Promise<Check> {
  try {
    const r = await exec(tool.name, tool.args, { cwd: process.cwd(), env: process.env, timeoutMs: 15000 });
    const out = `${r.stdout}\n${r.stderr}`;
    if (r.code !== 0) {
      return { name: tool.name, ok: false, expected: tool.expected, message: `${tool.name} ${tool.args.join(" ")} exited ${r.code}` };
    }
    const v = parseSemver(out);
    if (!v) return { name: tool.name, ok: false, expected: tool.expected, message: `no version in output: ${out.trim().slice(0, 200)}` };
    return { name: tool.name, ok: tool.accepts(v), found: v.join("."), expected: tool.expected };
  } catch (e) {
    return { name: tool.name, ok: false, expected: tool.expected, message: (e as Error).message };
  }
}

async function ghCheck(): Promise<Check> {
  try {
    const r = await exec("gh", ["--version"], { cwd: process.cwd(), env: process.env, timeoutMs: 15000 });
    const v = parseSemver(r.stdout);
    return r.code === 0
      ? { name: "gh", ok: true, found: v ? v.join(".") : r.stdout.split("\n")[0]!, expected: "present" }
      : { name: "gh", ok: false, expected: "present", message: `gh --version exited ${r.code}` };
  } catch (e) {
    return { name: "gh", ok: false, expected: "present", message: (e as Error).message };
  }
}

function nodeCheck(): Check {
  const v = parseSemver(process.versions.node)!;
  const stripping = Boolean(process.features.typescript);
  const ok = cmp(v, [22, 18, 0]) >= 0 && stripping;
  return {
    name: "node",
    ok,
    found: `${v.join(".")}, type stripping ${stripping ? process.features.typescript : "off"}`,
    expected: ">=22.18.0 with type stripping",
  };
}

const message = (e: unknown) => (e instanceof CpdError ? `${e.code}: ${e.message}` : (e as Error).message);

async function configAndTierChecks(): Promise<Check[]> {
  let config;
  try {
    config = loadConfig();
  } catch (e) {
    return [
      { name: "config", ok: false, message: message(e) },
      { name: "tiers", ok: false, expected: LETTERS.join(","), message: "not checked: config is not valid" },
    ];
  }
  const configOk: Check = { name: "config", ok: true, found: `version ${config.version}` };
  try {
    const catalog = await loadCatalog(config);
    return [configOk, { name: "tiers", ok: true, found: LETTERS.map((l) => `${l}=${catalog[l].model}`).join(" "), expected: LETTERS.join(",") }];
  } catch (e) {
    return [configOk, { name: "tiers", ok: false, expected: LETTERS.join(","), message: message(e) }];
  }
}

export async function run(argv: string[]): Promise<Result> {
  try {
    parseArgs({ args: argv, options: {}, allowPositionals: false, strict: true });
  } catch (e) {
    throw usage("usage", `cpd doctor: ${(e as Error).message}`);
  }
  const [tools, gh, cfg] = await Promise.all([Promise.all(TOOLS.map(toolCheck)), ghCheck(), configAndTierChecks()]);
  const checks = [nodeCheck(), ...tools, gh, ...cfg];
  const ok = checks.every((c) => c.ok);
  if (ok) return { ok: true, checks };
  // Contract §11 shape `{ok, checks}` plus the §2 failure envelope, exit 1.
  const failed = checks.filter((c) => !c.ok).map((c) => c.name);
  const body = {
    ok: false as const,
    checks,
    error: { code: "doctor_failed", message: `checks failed: ${failed.join(", ")}`, details: { failed } },
    exitCode: 1,
  };
  return body;
}
