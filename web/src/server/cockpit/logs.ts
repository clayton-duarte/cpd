export interface LogEntry { readonly id: number; readonly at: string; readonly kind: string; readonly text: string; readonly tone: "error" | "lead-error" | "normal" }
export interface LogsDeps { cpd(argv: readonly string[]): Promise<string> }
export async function readLogs(_deps: LogsDeps): Promise<LogEntry[]> {
  return [];
}
