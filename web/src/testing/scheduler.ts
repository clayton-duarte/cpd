import type { Disposable, Scheduler } from "../protocol/runtime.ts";

interface Timer {
  readonly seq: number;
  readonly due: number;
  readonly run: () => void;
}

/** A deterministic Scheduler: time moves only through `advance`. */
export class FakeScheduler implements Scheduler {
  private current = 0;
  private seq = 0;
  private timers: Timer[] = [];

  now(): number {
    return this.current;
  }

  after(ms: number, run: () => void): Disposable {
    const timer: Timer = { seq: this.seq++, due: this.current + ms, run };
    this.timers.push(timer);
    return {
      dispose: () => {
        this.timers = this.timers.filter((t) => t !== timer);
      },
    };
  }

  advance(ms: number): void {
    const target = this.current + ms;
    for (;;) {
      const next = this.timers
        .filter((t) => t.due <= target)
        .sort((a, b) => a.due - b.due || a.seq - b.seq)[0];
      if (next === undefined) break;
      this.timers = this.timers.filter((t) => t !== next);
      this.current = Math.max(this.current, next.due);
      next.run();
    }
    this.current = target;
  }

  pending(): number {
    return this.timers.length;
  }
}
