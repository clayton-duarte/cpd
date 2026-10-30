export type CommandUsage = { flags: Record<string, boolean>; positionals: { min: number; max: number | null }; required: string[] };
export type Grammar = Record<string, CommandUsage>;
export const grammar: Grammar = {};
export function usageError(_argv: string[], _g: Grammar = grammar): string | undefined {
  throw new Error("usageError is not implemented");
}
