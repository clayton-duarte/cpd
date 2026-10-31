import { join } from "node:path";
import { CpdError } from "../lib/cpd.ts";

export type Config = { home: string; session: string; cwd: string; model: string };

function flag(argv: readonly string[], name: string): string {
  const i = argv.indexOf(name);
  const v = i < 0 ? undefined : argv[i + 1];
  if (v === undefined || v === "") throw new CpdError("usage", `${name} <value> is required`);
  return v;
}

export function parseConfig(argv: readonly string[], env: Record<string, string | undefined>, cwd: string): Config {
  const session = flag(argv, "--session");
  const model = flag(argv, "--model");
  const home = env.CPD_HOME;
  if (home === undefined || home === "") throw new CpdError("usage", "CPD_HOME is not set");
  const bound = env.CPD_SESSION;
  if (bound === undefined || bound === "") throw new CpdError("usage", "CPD_SESSION is not set");
  if (bound !== session) throw new CpdError("usage", `CPD_SESSION ${bound} differs from --session ${session}`);
  return { home, session, cwd, model };
}

export function storePath(home: string, session: string): string {
  return join(home, "run", "host", `${session}.sqlite`);
}

export function socketPath(home: string, session: string): string {
  void home; void session;
  throw new Error("not implemented");
}
