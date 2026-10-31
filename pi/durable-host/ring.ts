import type { Cursor } from "./protocol.ts";

export type Frame =
  | { type: "snapshot"; epoch: string; seq: number; snapshot: unknown }
  | { type: "events"; epoch: string; seq: number; events: unknown[] };

export class Ring {
  constructor(_epoch: string, _snapshot: unknown) {
    throw new Error("not implemented");
  }
  get head(): number {
    throw new Error("not implemented");
  }
  append(_events: unknown[]): Frame {
    throw new Error("not implemented");
  }
  attach(_cursor?: Cursor): { mode: "snapshot" | "resume"; fromSeq: number; headSeq: number; frames: Frame[] } {
    throw new Error("not implemented");
  }
  shouldRebase(_runEnd: boolean): boolean {
    throw new Error("not implemented");
  }
  rebase(_snapshot: unknown): Frame {
    throw new Error("not implemented");
  }
}
