/** The one Tier parser: the model letter and the thinking effort are separate parts (bug B16). */
// Letters in ladder order: core tier.rs ranks J<G<H<S<O (contract §9); M,K,L,P,A follow them (guess: core ranks only the five).
export const MODEL_LETTERS = ["J", "G", "H", "S", "O", "M", "K", "L", "P", "A"] as const;
// Efforts in ladder order: 0<X<1<2 (core tier.rs effort_rank).
export const EFFORTS = ["0", "X", "1", "2"] as const;
export interface TierParts { readonly model: string; readonly effort: string }

const isLetter = (s: unknown): s is string => typeof s === "string" && (MODEL_LETTERS as readonly string[]).includes(s);
const isEffort = (s: unknown): s is string => typeof s === "string" && (EFFORTS as readonly string[]).includes(s);

/** A `{model, effort}` pair of catalog parts, or null. */
export function parseTierParts(v: unknown): TierParts | null {
  if (typeof v !== "object" || v === null || Array.isArray(v)) return null;
  const o = v as Record<string, unknown>;
  const keys = Object.keys(o);
  if (keys.length !== 2 || !isLetter(o["model"]) || !isEffort(o["effort"])) return null;
  return { model: o["model"], effort: o["effort"] };
}

/** A combined Tier string like `H1`, or null. */
export function parseTier(s: unknown): TierParts | null {
  if (typeof s !== "string" || s.length !== 2) return null;
  const model = s[0], effort = s[1];
  return isLetter(model) && isEffort(effort) ? { model: model as string, effort: effort as string } : null;
}

export const formatTier = (t: TierParts): string => `${t.model}${t.effort}`;

/** The place on the ladder, 0-based; null for an unparsable Tier. */
export function tierIndex(s: string): number | null {
  const t = parseTier(s);
  return t === null ? null : (MODEL_LETTERS as readonly string[]).indexOf(t.model) * EFFORTS.length + (EFFORTS as readonly string[]).indexOf(t.effort);
}
export const TIER_COUNT = MODEL_LETTERS.length * EFFORTS.length;
export const tierAt = (i: number): string => `${MODEL_LETTERS[Math.floor(i / EFFORTS.length)]}${EFFORTS[i % EFFORTS.length]}`;
