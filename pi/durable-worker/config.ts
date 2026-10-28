import { join } from "node:path";
import { CpdError } from "../lib/cpd.ts";

export type Config = { home: string; session: string; plan: string; attempt: string; cwd: string; spec: string; model: string };

const VARIABLES = ["CPD_HOME", "CPD_SESSION", "CPD_PLAN", "CPD_ATTEMPT"] as const;

function required(env: Record<string, string | undefined>, name: (typeof VARIABLES)[number]): string {
  const v = env[name];
  if (v === undefined || v === "") throw new CpdError("usage", `${name} is not set`);
  return v;
}

function flag(argv: readonly string[], name: string): string {
  const i = argv.indexOf(name);
  const v = i < 0 ? undefined : argv[i + 1];
  if (v === undefined || v === "") throw new CpdError("usage", `${name} <value> is required`);
  return v;
}

export function parseConfig(argv: readonly string[], env: Record<string, string | undefined>, cwd: string): Config {
  const [home, session, plan, attempt] = VARIABLES.map((n) => required(env, n)) as [string, string, string, string];
  return { home, session, plan, attempt, cwd, spec: flag(argv, "--spec"), model: flag(argv, "--model") };
}

export function storePath(home: string, plan: string, attempt: string): string {
  return join(home, "run", plan, "durable", `${attempt}.sqlite`);
}

export function reportPath(home: string, plan: string, attempt: string): string {
  return join(home, "run", plan, "durable", `${attempt}.report.json`);
}
