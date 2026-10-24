/**
 * i18n bridge for rpiv-ask-user-question — single thin import surface so every
 * call site routes through one place. cpd's copy has no i18n SDK: `t(key, fallback)` is an identity
 * passthrough that returns the inline English fallback at every call site.
 *
 * - `displayLabel(kind)` resolves a sentinel kind to its locale-aware label,
 *   with the canonical English `ROW_INTENT_META[kind].label` as fallback so
 *   nothing renders blank if the namespace isn't registered.
 *
 * Call sites
 * MUST use this module at render time — never bake the result into a top-level
 * `const X = displayLabel(...)`.
 *
 * Reserved-label validation stays English-locked: `RESERVED_LABEL_SET` checks
 * the canonical `ROW_INTENT_META[kind].label`, never `displayLabel(kind)`.
 */

import { ROW_INTENT_META, type SentinelKind } from "./row-intent.ts";

export const I18N_NAMESPACE = "@juicesharp/rpiv-ask-user-question";

type ScopeFn = (key: string, fallback: string) => string;

export const t: ScopeFn = (_key, fallback) => fallback;

export function displayLabel(kind: SentinelKind): string {
	return t(`sentinel.${kind}`, ROW_INTENT_META[kind].label);
}
