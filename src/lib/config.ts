import { readFileSync } from "node:fs";
import { z } from "zod";
import { configPath } from "./paths.ts";
import { failure } from "./errors.ts";

const tierSyntax = z.string().regex(/^[OSH][0-5]$/, "must be a Tier like S3");
/** `<provider>/<model-id>`, as pi addresses a model. */
const modelRef = z.string().regex(/^[^/\s]+\/\S+$/, "must be <provider>/<model-id>");

/** Contract §5. Strict: unknown keys are rejected. */
export const ConfigSchema = z.strictObject({
  version: z.literal(1),
  concurrency: z.number().int().min(1),
  retryCap: z.number().int().min(0),
  attemptTimeoutMinutes: z.number().positive(),
  askTimeoutMinutes: z.number().positive(),
  tickSeconds: z.number().positive(),
  prPollSeconds: z.number().positive(),
  herdrSession: z.string().min(1).nullable(),
  tiers: z.strictObject({ O: modelRef, S: modelRef, H: modelRef }),
  leadTier: tierSyntax,
  defaultReviewTier: tierSyntax,
  reviewMode: z.enum(["fork", "fresh"]),
  leadPiPackages: z.array(z.string().min(1)),
});

export type Config = z.infer<typeof ConfigSchema>;

/** Read and validate `$CPD_HOME/config.json`. */
export function loadConfig(): Config {
  const path = configPath();
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") {
      throw failure("config_missing", `config file not found: ${path}`, { path });
    }
    throw failure("config_unreadable", `cannot read config file ${path}: ${(e as Error).message}`, { path });
  }
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (e) {
    throw failure("config_invalid", `config file is not valid JSON: ${path}`, { path, parseError: (e as Error).message });
  }
  const parsed = ConfigSchema.safeParse(raw);
  if (!parsed.success) {
    throw failure("config_invalid", `config file does not match the schema: ${path}`, {
      path,
      issues: parsed.error.issues,
    });
  }
  return parsed.data;
}
