import { pathToFileURL } from "node:url";
import { z } from "zod";
import type { Config } from "./config.ts";
import { failure, usage } from "./errors.ts";
import { locatePi } from "./pi-install.ts";

// Contract §7.

export const LETTERS = ["O", "S", "H"] as const;
export type Letter = (typeof LETTERS)[number];
export type Effort = 0 | 1 | 2 | 3 | 4 | 5;
export type ParsedTier = { letter: Letter; effort: Effort };
/** Every function below takes either form; a string is parsed (and fails loud if malformed). */
export type TierInput = ParsedTier | string;

export type Catalog = Record<Letter, { model: string; efforts: number[] }>;

const LEVELS = ["off", "low", "medium", "high", "xhigh", "max"] as const;
export type Level = (typeof LEVELS)[number];

/** Model ladder, weakest to strongest. */
const MODEL_ORDER: readonly Letter[] = ["H", "S", "O"];

export function parseTier(s: string): ParsedTier {
  const m = typeof s === "string" ? /^([OSH])([0-5])$/.exec(s) : null;
  if (!m) throw usage("invalid_tier", `invalid Tier ${JSON.stringify(s)}: expected <O|S|H><0-5>, e.g. S3`, { tier: s });
  return { letter: m[1] as Letter, effort: Number(m[2]) as Effort };
}

const toTier = (t: TierInput): ParsedTier => (typeof t === "string" ? parseTier(t) : t);

export const formatTier = (t: TierInput): string => {
  const p = toTier(t);
  return `${p.letter}${p.effort}`;
};

/** Effort digit to pi thinking level (`minimal` is never used). */
export function levelForEffort(effort: number): Level {
  const level = LEVELS[effort];
  if (!Number.isInteger(effort) || level === undefined) {
    throw usage("invalid_effort", `invalid effort ${String(effort)}: expected an integer 0-5`, { effort });
  }
  return level;
}

function effortForLevel(level: string): number | null {
  const i = (LEVELS as readonly string[]).indexOf(level);
  return i === -1 ? null : i; // "minimal" has no effort digit
}

// --- catalog ---

const CatalogSchema = z.record(
  z.enum(LETTERS),
  z.strictObject({ model: z.string(), efforts: z.array(z.number().int().min(0).max(5)).min(1) }),
);
const LevelsSchema = z.array(z.string());

/** Load the Tier catalog from the installed pi release (contract §7; trial t6). */
export async function loadCatalog(config: Config): Promise<Catalog> {
  const pi = locatePi();
  const agent: any = await import(pathToFileURL(pi.codingAgentEntry).href);
  const ai: any = await import(pathToFileURL(pi.piAiEntry).href);
  if (typeof agent.ModelRuntime?.create !== "function") {
    throw failure("pi_sdk_unexpected", `pi ${pi.version}: ModelRuntime.create is missing`, { entry: pi.codingAgentEntry });
  }
  if (typeof ai.getSupportedThinkingLevels !== "function") {
    throw failure("pi_sdk_unexpected", `pi ${pi.version}: getSupportedThinkingLevels is missing`, { entry: pi.piAiEntry });
  }
  const runtime = await agent.ModelRuntime.create({ allowModelNetwork: false });
  const catalog: Partial<Catalog> = {};
  const missing: { letter: Letter; model: string }[] = [];
  for (const letter of LETTERS) {
    const ref = config.tiers[letter];
    const slash = ref.indexOf("/");
    const model = runtime.getModel(ref.slice(0, slash), ref.slice(slash + 1));
    if (!model) {
      missing.push({ letter, model: ref });
      continue;
    }
    const levels = LevelsSchema.parse(ai.getSupportedThinkingLevels(model));
    const efforts = levels.map(effortForLevel).filter((e): e is number => e !== null).sort((a, b) => a - b);
    catalog[letter] = { model: ref, efforts };
  }
  if (missing.length > 0) {
    throw failure(
      "tier_model_unknown",
      `model not in pi's catalog: ${missing.map((m) => `${m.letter}=${m.model}`).join(", ")}`,
      { missing, agentDir: pi.agentDir },
    );
  }
  return CatalogSchema.parse(catalog) as Catalog;
}

// --- validation and ladder ---

export function validTier(catalog: Catalog, tier: TierInput): boolean {
  const t = toTier(tier);
  return catalog[t.letter].efforts.includes(t.effort);
}

function assertValid(catalog: Catalog, tier: TierInput): ParsedTier {
  const t = toTier(tier);
  if (!validTier(catalog, t)) {
    throw usage(
      "tier_invalid",
      `Tier ${formatTier(t)} is not valid: ${catalog[t.letter].model} supports efforts ${catalog[t.letter].efforts.join(",")}`,
      { tier: formatTier(t), model: catalog[t.letter].model, efforts: catalog[t.letter].efforts },
    );
  }
  return t;
}

/** The `--model` argument for pi: `<provider/id>:<level>`. */
export function piModelArg(catalog: Catalog, tier: TierInput): string {
  const t = assertValid(catalog, tier);
  return `${catalog[t.letter].model}:${levelForEffort(t.effort)}`;
}

/** Ladder step: effort+1 on the same model if valid, else the next model (H→S→O) at its lowest valid effort ≥ the current one, else null. */
export function nextTier(catalog: Catalog, tier: TierInput): ParsedTier | null {
  const t = assertValid(catalog, tier);
  const up = { letter: t.letter, effort: (t.effort + 1) as Effort };
  if (up.effort <= 5 && validTier(catalog, up)) return up;
  for (const letter of MODEL_ORDER.slice(MODEL_ORDER.indexOf(t.letter) + 1)) {
    const effort = catalog[letter].efforts.find((e) => e >= t.effort);
    if (effort !== undefined) return { letter, effort: effort as Effort };
  }
  return null;
}

/** Attempt 1 uses the Job's Tier; attempt n applies `nextTier` n-1 times. Null when the ladder runs out first. */
export function attemptTier(catalog: Catalog, jobTier: TierInput, n: number): ParsedTier | null {
  if (!Number.isInteger(n) || n < 1) throw usage("invalid_attempt_number", `invalid attempt number: ${String(n)}`, { n });
  let t: ParsedTier | null = assertValid(catalog, jobTier);
  for (let i = 1; i < n && t; i++) t = nextTier(catalog, t);
  return t;
}
