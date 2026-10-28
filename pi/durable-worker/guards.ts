export type Verdict = { block: string } | undefined;

export function workerVerdict(_call: { name: string; arguments: Record<string, unknown> }, _where: { cwd: string; home: string }): Verdict {
  throw new Error("not implemented");
}
