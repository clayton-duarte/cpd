import type { HostCursor } from "./client.ts";

export type Op = readonly unknown[];
export interface Replica {
  view(frame: { epoch: string; seq: number; view: unknown }): void;
  ops(frame: { epoch: string; seq: number; ops: readonly unknown[] }): void;
  readonly value: unknown | null;
  readonly cursor: HostCursor | null;
}

export function applyOps(_root: unknown, _ops: readonly unknown[]): unknown {
  throw new Error("not implemented");
}

export function createReplica(): Replica {
  throw new Error("not implemented");
}
