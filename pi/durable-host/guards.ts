export type Where = { cwd: string; home: string };

export type Verdict = { block: string } | undefined;

export function hostVerdict(_call: { name: string; arguments: Record<string, unknown> }, _where: Where): Verdict {
  throw new Error("not implemented");
}
