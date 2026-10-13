import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { loadConfig } from "../lib/config.ts";
import { failure, refused, usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";
import { cpdHome } from "../lib/paths.ts";
import { locatePi } from "../lib/pi-install.ts";
import { loadCatalog, piModelArg } from "../lib/tiers.ts";

/** The cpd repository root (this file is `<root>/src/commands/lead-start.ts`); the pi adapter package is `<root>/pi`. */
const REPO_ROOT = fileURLToPath(new URL("../../", import.meta.url));

/**
 * Contract §11: replace this process with `pi --model <leadTier> -e <repo>/pi -e <each leadPiPackages>`.
 *
 * Mechanism: `process.execve` (Node >= 22.15; `package.json` requires >= 22.18, so it is always present and
 * there is no spawn fallback). It never returns, and a failing execve aborts the process instead of throwing,
 * so the executable is resolved to an existing path first (`locatePi` checks it is executable).
 * stdin/stdout/stderr are inherited, so pi owns the herdr pane's terminal.
 */
export async function run(argv: string[]): Promise<Result> {
  try {
    parseArgs({ args: argv, options: {}, allowPositionals: false, strict: true });
  } catch (e) {
    throw usage("usage", `cpd lead start: ${(e as Error).message}`);
  }
  if (process.env.HERDR_ENV !== "1") {
    throw refused("not_in_herdr", "cpd lead start must run inside a herdr pane (HERDR_ENV=1)");
  }
  const config = loadConfig();
  const model = piModelArg(await loadCatalog(config), config.leadTier);
  const pi = locatePi();
  const home = cpdHome();
  const args = ["--model", model, "-e", join(REPO_ROOT, "pi"), ...config.leadPiPackages.flatMap((p) => ["-e", p])];
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    CPD_HOME: home,
    PLANNOTATOR_DATA_DIR: join(home, "plannotator"),
    PLANNOTATOR_UPDATE_CHECK: "0",
  };
  if (typeof process.execve !== "function") {
    throw failure("execve_unavailable", "process.execve is not available in this Node; need >= 22.15");
  }
  return process.execve(pi.piPath, ["pi", ...args], env);
}
