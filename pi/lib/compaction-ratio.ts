import { AgentSession, SettingsManager } from "@earendil-works/pi-coding-agent";
import { calculateModelInputCap } from "./model-input-cap.ts";
import { hookMethod, type MethodBehavior } from "../compact-tools/extensions/compact-tools-hook.ts";

/** Derive the reserve from the active model's input cap, ignoring legacy settings. */
export interface ReserveModel {
  provider: string;
  modelId: string;
  contextWindow?: number;
}

export function reserveTokensOf(_compaction: unknown, model: ReserveModel | undefined): number {
  const window = model?.contextWindow;
  const cap = calculateModelInputCap(window, model ? `${model.provider}/${model.modelId}` : "unknown");
  return (window as number) - cap;
}

const behavior: MethodBehavior = (_self, args) => {
  const model = args[0] as { provider?: string; id?: string; contextWindow?: number } | undefined;
  const window = model?.contextWindow;
  const cap = calculateModelInputCap(window, model ? `${model.provider}/${model.id}` : "unknown");
  return (window as number) - cap;
};

/**
 * Idle-only threshold compaction (docs/commands.md section 15.19): installs its wrappers on `target`.
 * In the post-run check, pi has already set `agent.state.isStreaming` false once `agent.prompt` resolved,
 * so the wrapped `_exceedsCompactionThreshold` answers truthfully there.
 */
export function hookIdleCompaction(target: object): void {
	hookMethod(target, "_compactBeforeNextAssistantResponse", "cpd-compaction-idle-before-response", (self, args) =>
		Promise.resolve({ ...args[0], messages: self.sessionManager.buildSessionProjection().messages }),
	);
	hookMethod(target, "_exceedsCompactionThreshold", "cpd-compaction-idle-threshold", (self, args, original) =>
		self.agent?.state?.isStreaming === true ? false : original.apply(self, args),
	);
	hookMethod(target, "_checkCompaction", "cpd-compaction-idle-check", async (self, args, original) => {
		const assistantMessage = args[0];
		if (self.autoCompactionEnabled === false) return false;
		if (args[1] === false) return false;
		if (self.agent?.hasQueuedMessages?.() === true) {
			return assistantMessage?.stopReason === "error" ? original.apply(self, args) : false;
		}
		const compacted = await original.apply(self, args);
		const stop = assistantMessage?.stopReason;
		if (
			!compacted &&
			stop !== "error" &&
			stop !== "aborted" &&
			self.model &&
			self._exceedsCompactionThreshold(self.model, self.sessionManager.buildSessionProjection())
		) {
			return await self._runAutoCompaction("threshold", false);
		}
		return compacted;
	});
}

/** Wraps `getCompactionReserveTokens` once per process, as the adapter loads. */
export function registerCompactionRatio(): void {
	hookIdleCompaction(AgentSession.prototype);
	hookMethod(SettingsManager.prototype, "getCompactionReserveTokens", "cpd-compaction-ratio", behavior);
}
