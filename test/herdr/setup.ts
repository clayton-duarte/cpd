import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fakeBin, sandboxEnv, type FakeBin } from "../helpers/index.ts";

export const baseConfig = {
  version: 1, concurrency: 3, retryCap: 2, attemptTimeoutMinutes: 30, askTimeoutMinutes: 60, tickSeconds: 5,
  prPollSeconds: 120, herdrSession: null as string | null,
  tiers: { O: "github-copilot/claude-opus-5.5", S: "github-copilot/claude-sonnet-5.5", H: "github-copilot/claude-haiku-4.5" },
  leadTier: "O3", defaultReviewTier: "S4", reviewMode: "fork", leadPiPackages: [] as string[],
};

const ORIGINAL_PATH = process.env.PATH ?? "";

/** Write a config under a sandboxed CPD_HOME and put a fake `herdr` (running `script`) first on PATH, in this process. */
export function useFakeHerdr(script: string, herdrSession: string | null = null): FakeBin {
  const e = sandboxEnv();
  mkdirSync(e.CPD_HOME!, { recursive: true });
  writeFileSync(join(e.CPD_HOME!, "config.json"), JSON.stringify({ ...baseConfig, herdrSession }));
  process.env.CPD_HOME = e.CPD_HOME;
  const bin = fakeBin("herdr", script);
  process.env.PATH = `${bin.dir}:${ORIGINAL_PATH}`;
  return bin;
}

/** Shell snippet printing `json` on stdout (a herdr success or error envelope). */
export const says = (json: unknown) => `printf '%s\\n' '${JSON.stringify(json).replaceAll("'", "'\\''")}'`;
