/**
 * The model input cap (pro-d559, contract §6.22, docs/architecture/model-input-cap.md).
 *
 * Hard ceiling on model input:
 * cap = min(200000, floor(0.8 * W)) tokens, where W is the active model's context window.
 * Computed in integers as floor(W * 4 / 5).
 */

export class ModelInputCapError extends Error {
  readonly code: string;
  readonly exitCode: number;
  readonly details: Record<string, unknown>;

  constructor(code: string, exitCode: number, message: string, details: Record<string, unknown> = {}) {
    super(message);
    this.name = "ModelInputCapError";
    this.code = code;
    this.exitCode = exitCode;
    this.details = details;
  }
}

export class ContextWindowInvalidError extends ModelInputCapError {
  constructor(model: string, value: unknown) {
    super(
      "context_window_invalid",
      1,
      `Invalid context window for model ${model}: ${String(value)}`,
      { model, value }
    );
    this.name = "ContextWindowInvalidError";
  }
}

export class InputOverCapError extends ModelInputCapError {
  constructor(model: string, window: number, cap: number, tokens: number) {
    super(
      "input_over_cap",
      3,
      `Model input over cap for model ${model}: ${tokens} tokens > cap ${cap} (window ${window})`,
      { model, window, cap, tokens }
    );
    this.name = "InputOverCapError";
  }
}

export interface ModelMetadata {
  provider?: string;
  id?: string;
  contextWindow?: unknown;
}

export interface InputBreakdown {
  systemPromptTokens?: number;
  toolSchemaTokens?: number;
  messageTokens?: number;
  toolResultTokens?: number;
  totalTokens?: number;
}

export interface DispatchRequest<TMessage = unknown> {
  model: ModelMetadata;
  breakdown?: InputBreakdown;
  messages?: TMessage[];
  totalTokens?: number;
}

export interface DispatchGuardContext<TMessage = unknown> {
  compact: (messages: TMessage[]) => Promise<{ compactedMessages: TMessage[]; compactedTokens: number }> | { compactedMessages: TMessage[]; compactedTokens: number };
  send: (request: DispatchRequest<TMessage>) => Promise<unknown> | unknown;
}

/**
 * Validates the context window W and computes the hard input cap:
 * cap = min(200000, floor(0.8 * W)) tokens (calculated as floor(W * 4 / 5)).
 *
 * Throws ContextWindowInvalidError if W is not a finite positive integer.
 */
export function calculateModelInputCap(window: unknown, modelId = "unknown"): number {
  if (
    typeof window !== "number" ||
    !Number.isFinite(window) ||
    !Number.isInteger(window) ||
    window <= 0
  ) {
    throw new ContextWindowInvalidError(modelId, window);
  }

  const derived = Math.floor((window * 4) / 5);
  return Math.min(200_000, derived);
}

/**
 * Computes effective input tokens from breakdown or totalTokens.
 */
function computeEffectiveTokens(breakdown?: InputBreakdown, totalTokens?: number): number {
  if (breakdown !== undefined) {
    return (
      (breakdown.systemPromptTokens ?? 0) +
      (breakdown.toolSchemaTokens ?? 0) +
      (breakdown.messageTokens ?? 0) +
      (breakdown.toolResultTokens ?? 0) +
      (breakdown.totalTokens ?? 0)
    );
  }
  return totalTokens ?? 0;
}

/**
 * Measures the effective input and guards model dispatch:
 * 1. Resolves and validates window W;
 * 2. Counts full effective input (system prompt, tool schemas, messages, tool results);
 * 3. If tokens <= cap, dispatches;
 * 4. Otherwise triggers one compaction, re-measures;
 * 5. If tokens <= cap, dispatches rebuilt input;
 * 6. Otherwise refuses with InputOverCapError (exit 3) and sends NO request.
 */
export async function guardModelDispatch<TMessage = unknown>(
  request: DispatchRequest<TMessage>,
  context: DispatchGuardContext<TMessage>
): Promise<unknown> {
  const modelId = request.model?.id ?? "unknown";
  const window = request.model?.contextWindow;

  // 1. Resolve and validate window W
  const cap = calculateModelInputCap(window, modelId);
  const validWindow = window as number;

  // 2. Count full effective input
  let effectiveTokens = computeEffectiveTokens(request.breakdown, request.totalTokens);

  // 3. If tokens <= cap, dispatch
  if (effectiveTokens <= cap) {
    return await context.send(request);
  }

  // 4. Otherwise trigger one compaction, re-measure
  const messagesToCompact = request.messages ? [...request.messages] : [];
  const compactionResult = await context.compact(messagesToCompact);

  const compactedMessages = compactionResult.compactedMessages;
  const compactedTokens = compactionResult.compactedTokens;

  // 5. If tokens <= cap, dispatch rebuilt input
  if (compactedTokens <= cap) {
    const updatedRequest: DispatchRequest<TMessage> = {
      ...request,
      messages: compactedMessages,
      totalTokens: compactedTokens,
      breakdown: undefined, // compacted tokens represents the rebuilt effective input
    };
    return await context.send(updatedRequest);
  }

  // 6. Otherwise refuse with InputOverCapError (exit 3) and send NO request
  throw new InputOverCapError(modelId, validWindow, cap, compactedTokens);
}
