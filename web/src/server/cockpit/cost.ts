export interface CostFile { readonly text: string; readonly mtimeMs: number; readonly size: number }
export interface CostDeps {
  cpd(argv: readonly string[]): Promise<string>;
  readFile(path: string): Promise<CostFile | null>;
}
export async function readCosts(_deps: CostDeps): Promise<Record<string, number>> {
  return {};
}
export function formatCost(_n: number): string {
  return "";
}
