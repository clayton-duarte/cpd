import { parseArgs } from "node:util";
import { loadConfig } from "../lib/config.ts";
import { usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";
import { loadCatalog } from "../lib/tiers.ts";

export async function run(argv: string[]): Promise<Result> {
  try {
    parseArgs({ args: argv, options: {}, allowPositionals: false, strict: true });
  } catch (e) {
    throw usage("usage", `cpd tiers: ${(e as Error).message}`);
  }
  const catalog = await loadCatalog(loadConfig());
  return { ok: true, tiers: catalog };
}
