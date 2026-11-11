/**
 * The model input cap (pro-d559, contract §6.22, docs/architecture/model-input-cap.md).
 *
 * Hard ceiling on model input:
 * cap = min(200000, floor(0.8 * W)) tokens, where W is the active model's context window.
 *
 * This file declares the interface, error types, and stubs for cap calculation and
 * pre-dispatch enforcement. Implementation is completed in cap-impl.
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
  throw new Error("stub: calculateModelInputCap not implemented");
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
  throw new Error("stub: guardModelDispatch not implemented");
}
