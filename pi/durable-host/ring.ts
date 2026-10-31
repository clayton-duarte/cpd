import { type Cursor, LIMITS } from "./protocol.ts";

export type Frame =
  | { type: "snapshot"; epoch: string; seq: number; snapshot: unknown }
  | { type: "events"; epoch: string; seq: number; events: unknown[] };

export class Ring {
  #epoch: string;
  #base: Extract<Frame, { type: "snapshot" }>;
  #frames: Extract<Frame, { type: "events" }>[] = [];

  constructor(epoch: string, snapshot: unknown) {
    this.#epoch = epoch;
    this.#base = { type: "snapshot", epoch, seq: 1, snapshot };
  }

  get head(): number {
    return this.#base.seq + this.#frames.length;
  }

  append(events: unknown[]): Frame {
    const frame = { type: "events" as const, epoch: this.#epoch, seq: this.head + 1, events };
    this.#frames.push(frame);
    return frame;
  }

  attach(cursor?: Cursor): { mode: "snapshot" | "resume"; fromSeq: number; headSeq: number; frames: Frame[] } {
    const headSeq = this.head;
    const baseSeq = this.#base.seq;
    if (cursor && cursor.epoch === this.#epoch && cursor.seq >= baseSeq && cursor.seq <= headSeq) {
      return { mode: "resume", fromSeq: cursor.seq + 1, headSeq, frames: this.#frames.slice(cursor.seq - baseSeq) };
    }
    return { mode: "snapshot", fromSeq: baseSeq, headSeq, frames: [this.#base, ...this.#frames] };
  }

  shouldRebase(runEnd: boolean): boolean {
    const n = this.#frames.length;
    return n > LIMITS.rebaseMax || (runEnd && n > LIMITS.rebaseAtRunEnd);
  }

  rebase(snapshot: unknown): Frame {
    this.#base = { type: "snapshot", epoch: this.#epoch, seq: this.head + 1, snapshot };
    this.#frames = [];
    return this.#base;
  }
}
