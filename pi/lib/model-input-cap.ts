/**
 * Stub for model input cap (pro-d559).
 * Real implementation will be provided in cap-impl.
 */

export interface ModelInputCapModel {
  provider?: string;
  id?: string;
  contextWindow?: number;
}

export interface ModelInputCapOptions {
  model?: ModelInputCapModel;
  systemPromptTokens?: number;
  toolSchemaTokens?: number;
  messageTokens?: number;
  toolResultTokens?: number;
  totalTokens?: number;
  messages?: unknown[];
}

export function calculateModelInputCap(window: unknown): number {
  throw new Error("stub: calculateModelInputCap not implemented");
}

export function enforceModelInputCap(options: unknown): unknown {
  throw new Error("stub: enforceModelInputCap not implemented");
}
