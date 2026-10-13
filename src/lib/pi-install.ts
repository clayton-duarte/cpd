import { accessSync, constants, existsSync, readFileSync, realpathSync, statSync } from "node:fs";
import { delimiter, dirname, join } from "node:path";
import { failure } from "./errors.ts";

export type PiInstall = {
  /** The `pi` found on PATH, as found. */
  piPath: string;
  agentDir: string;
  version: string;
  releaseDir: string;
  /** `@earendil-works/pi-coding-agent` entry point inside the release. */
  codingAgentEntry: string;
  /** `@earendil-works/pi-ai` entry point inside the same release. */
  piAiEntry: string;
};

/** The same character set the launcher accepts for `current-version`. */
const VERSION = /^[0-9A-Za-z._+-]+$/;

function findOnPath(name: string, pathVar: string): string | null {
  for (const dir of pathVar.split(delimiter)) {
    if (!dir) continue;
    const candidate = join(dir, name);
    try {
      if (!statSync(candidate).isFile()) continue;
      accessSync(candidate, constants.X_OK);
      return candidate;
    } catch {
      // not here
    }
  }
  return null;
}

const notFound = (message: string, details: Record<string, unknown>) => failure("pi_not_found", message, details);

/** Contract §7: realpath of `pi` on PATH → agent dir → `install/current-version` → release dir. */
export function locatePi(env: NodeJS.ProcessEnv = process.env): PiInstall {
  const piPath = findOnPath("pi", env.PATH ?? "");
  if (!piPath) throw notFound("`pi` is not on PATH", { path: env.PATH ?? "" });
  const real = realpathSync(piPath);
  const agentDir = dirname(dirname(real));
  const currentFile = join(agentDir, "install", "current-version");
  let version: string;
  try {
    version = readFileSync(currentFile, "utf8").split("\n")[0]!.trim();
  } catch (e) {
    throw notFound(`cannot read ${currentFile}: ${(e as Error).message}`, { piPath, real, currentFile });
  }
  if (version === "." || version === ".." || !VERSION.test(version)) {
    throw notFound(`invalid pi version in ${currentFile}: ${JSON.stringify(version)}`, { currentFile, version });
  }
  const releaseDir = join(agentDir, "install", "releases", version);
  const scope = join(releaseDir, "node_modules", "@earendil-works");
  const codingAgentEntry = join(scope, "pi-coding-agent", "dist", "index.js");
  const piAiEntry = join(scope, "pi-ai", "dist", "index.js");
  for (const f of [codingAgentEntry, piAiEntry]) {
    if (!existsSync(f)) throw notFound(`pi release ${version} is incomplete: missing ${f}`, { releaseDir, missing: f });
  }
  return { piPath, agentDir, version, releaseDir, codingAgentEntry, piAiEntry };
}
