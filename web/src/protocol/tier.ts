export interface TierParts { readonly model: string; readonly effort: string }

/** A `{model, effort}` pair of parts, or null. Model is non-empty string, effort is non-empty string. */
export function parseTierParts(v: unknown): TierParts | null {
  if (typeof v !== "object" || v === null || Array.isArray(v)) return null;
  const o = v as Record<string, unknown>;
  const keys = Object.keys(o);
  if (keys.length !== 2 || typeof o["model"] !== "string" || o["model"] === "" || typeof o["effort"] !== "string" || o["effort"] === "") return null;
  return { model: o["model"], effort: o["effort"] };
}

export const formatTier = (t: TierParts): string => `${t.model}${t.effort}`;
