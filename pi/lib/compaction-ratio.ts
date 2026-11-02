import { AgentSession, SettingsManager } from "@earendil-works/pi-coding-agent";
import { hookMethod, type MethodBehavior } from "../compact-tools/extensions/compact-tools-hook.ts";

/**
 * docs/commands.md section 15.19 The compaction ratio: pi's flat `compaction.reserveTokens` makes a
 * large-window model compact only near its limit, so the reserve is derived from the model in hand as
 * `Math.round(contextWindow * ratio)`. The module wraps pi's own
 * `SettingsManager.prototype.getCompactionReserveTokens`; the value is derived on every call and never
 * stored. Precedence, per model key `provider/modelId`, highest first: the model override's
 * `reserveTokens` (explicit absolute), the model override's `reserveTokensRatio`, the global
 * `reserveTokensRatio`, the global `reserveTokens` (explicit absolute, only when no ratio applies),
 * then the built-in ratio `0.1`. A model with no positive contextWindow keeps pi's value; a ratio
 * outside (0, 1) or not finite throws naming the exact key, with no silent fallback.
 */

/** The `compaction` block cpd reads. pi's own schema does not declare the ratio; the settings file is the source. */
interface CompactionSettings {
	reserveTokens?: number;
	reserveTokensRatio?: number;
	modelOverrides?: Record<string, { reserveTokens?: number; reserveTokensRatio?: number } | undefined>;
}

/** The model pi passes: its declared type is narrower than the object the runtime carries. */
interface CompactionModel {
	provider?: string;
	id?: string;
	contextWindow?: number;
}

const behavior: MethodBehavior = (self, args, original) => {
	const model = args[0] as CompactionModel | undefined;
	const settings = (self.getSettings?.() ?? {}) as { compaction?: CompactionSettings };
	const compaction = settings.compaction ?? {};
	const key = model ? `${model.provider}/${model.id}` : undefined;
	const override = key ? compaction.modelOverrides?.[key] : undefined;

	// An explicit absolute override wins over every ratio.
	if (override && Object.hasOwn(override, "reserveTokens")) return original.apply(self, args);

	// A present ratio is authoritative (a JSON `null` from a NaN still names its key); a present
	// global reserveTokens suppresses the built-in ratio; otherwise the built-in 0.1 applies.
	let ratio: unknown;
	let name: string;
	if (override && Object.hasOwn(override, "reserveTokensRatio")) {
		ratio = override.reserveTokensRatio;
		name = `compaction.modelOverrides["${key}"].reserveTokensRatio`;
	} else if (Object.hasOwn(compaction, "reserveTokensRatio")) {
		ratio = compaction.reserveTokensRatio;
		name = "compaction.reserveTokensRatio";
	} else if (Object.hasOwn(compaction, "reserveTokens")) {
		return original.apply(self, args);
	} else {
		ratio = 0.1;
		name = "the built-in compaction ratio";
	}

	if (typeof ratio !== "number" || !Number.isFinite(ratio) || ratio <= 0 || ratio >= 1) {
		throw new Error(`Invalid ${name} setting: ${String(ratio)}. Expected a finite number between 0 and 1.`);
	}

	const window = model?.contextWindow;
	if (typeof window === "number" && Number.isFinite(window) && window > 0) return Math.round(window * ratio);
	return original.apply(self, args);
};

/** The model `reserveTokensOf` derives from: the `provider/modelId` key and its declared context window. */
export interface ReserveModel {
  provider: string;
  modelId: string;
  contextWindow?: number;
}

/**
 * The pure derivation of the compaction reserve (contract §6.18): the same precedence the prototype hook applies,
 * `Math.round(contextWindow * ratio)`, and the same throws. One source of truth the hook and the durable host share.
 */
export function reserveTokensOf(compaction: CompactionSettings, model: ReserveModel | undefined): number | undefined {
  const key = model ? `${model.provider}/${model.modelId}` : undefined;
  const override = key ? compaction.modelOverrides?.[key] : undefined;

  // An explicit absolute override wins over every ratio.
  if (override && Object.hasOwn(override, "reserveTokens")) return override.reserveTokens;

  // A present ratio is authoritative; a present global reserveTokens suppresses the built-in ratio.
  let ratio: unknown;
  let name: string;
  if (override && Object.hasOwn(override, "reserveTokensRatio")) {
    ratio = override.reserveTokensRatio;
    name = `compaction.modelOverrides["${key}"].reserveTokensRatio`;
  } else if (Object.hasOwn(compaction, "reserveTokensRatio")) {
    ratio = compaction.reserveTokensRatio;
    name = "compaction.reserveTokensRatio";
  } else if (Object.hasOwn(compaction, "reserveTokens")) {
    return compaction.reserveTokens;
  } else {
    ratio = 0.1;
    name = "the built-in compaction ratio";
  }

  if (typeof ratio !== "number" || !Number.isFinite(ratio) || ratio <= 0 || ratio >= 1) {
    throw new Error(`Invalid ${name} setting: ${String(ratio)}. Expected a finite number between 0 and 1.`);
  }

  const window = model?.contextWindow;
  if (typeof window === "number" && Number.isFinite(window) && window > 0) return Math.round(window * ratio);
  return compaction.reserveTokens;
}

/** Idle-only threshold compaction (docs/commands.md section 15.19): installs its wrappers on `target`. Stub. */
export function hookIdleCompaction(_target: object): void {}

/** Wraps `getCompactionReserveTokens` once per process, as the adapter loads. */
export function registerCompactionRatio(): void {
	hookIdleCompaction(AgentSession.prototype);
	hookMethod(SettingsManager.prototype, "getCompactionReserveTokens", "cpd-compaction-ratio", behavior);
}
