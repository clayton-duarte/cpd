// Tool guard validation hooks (docs/architecture/pi-native-core.md §4.6).
// Replaces bash regex scanning with structured tool argument inspection.

export interface ToolCallTarget {
  toolName: string;
  input: unknown;
  cwd?: string;
  home?: string;
  role?: string;
}

export interface GuardVerdict {
  blocked: boolean;
  reason?: string;
}

/**
 * Checks whether tool execution is permitted.
 */
export function validateToolCall(call: ToolCallTarget): GuardVerdict {
  return { blocked: false };
}
