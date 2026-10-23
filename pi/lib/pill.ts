// STUB: red-test Job. Placeholder values are wrong on purpose.
export type MeaningName = "plain" | "in_progress" | "review" | "done" | "failed" | "planning" | "nothing" | "focus";
export interface PillSpec {
  caps: { left: string; right: string };
  clickable: { add: number; from: number; to: number };
  meanings: Record<MeaningName, number>;
}
export function parseSpec(_raw: unknown): PillSpec {
  throw new Error("not implemented");
}
export const spec: PillSpec = {
  caps: { left: "?", right: "?" },
  clickable: { add: 0, from: 0, to: 0 },
  meanings: { plain: 0, in_progress: 0, review: 0, done: 0, failed: 0, planning: 0, nothing: 0, focus: 0 },
};
export function foreground(_index: number): string {
  throw new Error("not implemented");
}
export function pill(_index: number, _text: string): string {
  throw new Error("not implemented");
}
export function clickable(_index: number): number {
  throw new Error("not implemented");
}
export function meaning(_name: MeaningName): number {
  throw new Error("not implemented");
}
