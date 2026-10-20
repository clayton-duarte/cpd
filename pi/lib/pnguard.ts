// Stub for the guard of commands.md §15 "Every plannotator session is tracked" (G1-G3); the logic is not written yet.

export type GuardState = {
  env: Record<string, string | undefined>;
  /** A live cpd run exists (start-up finished). */
  live: boolean;
  /** Where the registry lives (`globalThis`). */
  store: Record<string, unknown>;
};

export type GuardCall = { toolName: string; input: unknown; parentToolCallId?: string };

export function pnVerdict(_state: GuardState, _call: GuardCall): { block: true; reason: string } | undefined {
  throw new Error("not implemented");
}
