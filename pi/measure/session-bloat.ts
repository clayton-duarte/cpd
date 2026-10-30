export type Usage = { input: number; output: number; cache_read: number; cache_write: number; cost_usd: number };
export type Row = { tool: string; calls: number; chars: number };
export type CommandRow = { key: string; calls: number; chars: number };
export type Largest = { chars: number; tool: string; command: string; file: string; entry: string };
export type RoleReport = {
  sessions: number;
  turns: number;
  tool_results: number;
  result_chars: number;
  result_tokens_est: number;
  usage: Usage;
  by_tool: Row[];
  top_commands: CommandRow[];
  largest: Largest[];
  codemode: { calls: number; output_chars: number; nested: { name: string; calls: number }[] };
};
/** The JSON report of docs/implementing.md, `Measuring tool-result bloat`. */
export type Report = {
  window: { since: string | null; until: string | null };
  files: number;
  skipped_lines: number;
  roles: { worker: RoleReport; manager: RoleReport; lead: RoleReport; other: RoleReport };
};

/** Pure: the contents of pi session files in, the bloat report out. */
export function report(
  _files: { name: string; text: string }[],
  _options: { since?: string; until?: string; top?: number },
): Report {
  throw new Error("not implemented");
}

/** The command: parses options, reads `*.jsonl` one directory below the root, prints, returns the exit code. */
export function main(_argv: string[], _io: { out(text: string): void; err(text: string): void; home: string }): number {
  throw new Error("not implemented");
}
