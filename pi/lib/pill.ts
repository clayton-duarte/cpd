import { readFileSync } from "node:fs";

export type MeaningName = "plain" | "in_progress" | "review" | "done" | "failed" | "planning" | "nothing" | "focus";
export interface PillSpec {
  caps: { left: string; right: string };
  clickable: { add: number; from: number; to: number };
  meanings: Record<MeaningName, number>;
}

function isIndex(v: unknown): v is number {
  return typeof v === "number" && Number.isInteger(v) && v >= 1 && v <= 15;
}

function obj(raw: unknown, name: string, keys: string[]): Record<string, unknown> {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) throw new Error(`${name}: expected an object`);
  const o = raw as Record<string, unknown>;
  for (const k of Object.keys(o)) if (!keys.includes(k)) throw new Error(`${name}: unknown key ${k}`);
  for (const k of keys) if (!(k in o)) throw new Error(`${name}: missing key ${k}`);
  return o;
}

export function parseSpec(raw: unknown): PillSpec {
  const top = obj(raw, "spec", ["caps", "clickable", "meanings"]);
  const caps = obj(top.caps, "caps", ["left", "right"]);
  for (const k of ["left", "right"]) if (typeof caps[k] !== "string") throw new Error(`caps.${k}: expected a string`);
  const cl = obj(top.clickable, "clickable", ["add", "from", "to"]);
  for (const k of ["add", "from", "to"]) if (!isIndex(cl[k])) throw new Error(`clickable.${k}: not an integer in 1-15`);
  const names = ["plain", "in_progress", "review", "done", "failed", "planning", "nothing", "focus"];
  const m = obj(top.meanings, "meanings", names);
  for (const k of names) if (!isIndex(m[k])) throw new Error(`meanings.${k}: not an integer in 1-15`);
  return raw as PillSpec;
}

export const spec: PillSpec = parseSpec(JSON.parse(readFileSync(new URL("./pill-spec.json", import.meta.url), "utf8")));

export function foreground(index: number): string {
  if (!isIndex(index)) throw new Error(`index ${index}: not an integer in 1-15`);
  return index < 8 ? `\x1b[3${index}m` : `\x1b[9${index - 8}m`;
}

export function pill(index: number, text: string): string {
  const fg = foreground(index);
  return `${fg}${spec.caps.left}\x1b[7m${text}\x1b[27m${fg}${spec.caps.right}\x1b[39m`;
}

export function clickable(index: number): number {
  const c = spec.clickable;
  return index >= c.from && index <= c.to ? index + c.add : index;
}

export function meaning(name: MeaningName): number {
  return spec.meanings[name];
}
