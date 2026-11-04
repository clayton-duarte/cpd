import { type Cursor, LIMITS } from "./protocol.ts";

export type Frame =
  | { type: "view"; epoch: string; seq: number; view: unknown }
  | { type: "ops"; epoch: string; seq: number; ops: unknown[] };

export class Ring {
  #epoch: string;
  #base: Extract<Frame, { type: "view" }>;
  #frames: Extract<Frame, { type: "ops" }>[] = [];

  constructor(epoch: string, view: unknown) {
    this.#epoch = epoch;
    this.#base = { type: "view", epoch, seq: 1, view };
  }

  get head(): number {
    return this.#base.seq + this.#frames.length;
  }

  append(ops: unknown[]): Frame {
    const frame: Extract<Frame, { type: "ops" }> = { type: "ops", epoch: this.#epoch, seq: this.head + 1, ops };
    this.#frames.push(frame);
    return frame;
  }

  attach(cursor?: Cursor): { mode: "view" | "resume"; fromSeq: number; headSeq: number; frames: Frame[] } {
    const headSeq = this.head;
    const baseSeq = this.#base.seq;
    if (cursor && cursor.epoch === this.#epoch && cursor.seq >= baseSeq && cursor.seq <= headSeq) {
      return { mode: "resume", fromSeq: cursor.seq + 1, headSeq, frames: this.#frames.slice(cursor.seq - baseSeq) };
    }
    return { mode: "view", fromSeq: baseSeq, headSeq, frames: [this.#base, ...this.#frames] };
  }

  shouldRebase(runEnd: boolean): boolean {
    const n = this.#frames.length;
    return n > LIMITS.rebaseMax || (runEnd && n > LIMITS.rebaseAtRunEnd);
  }

  rebase(view: unknown): Frame {
    this.#base = { type: "view", epoch: this.#epoch, seq: this.head + 1, view };
    this.#frames = [];
    return this.#base;
  }
}
