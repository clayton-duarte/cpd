import type { Disposable, Scheduler } from "../protocol/runtime.ts";

export class FakeScheduler implements Scheduler {
  now(): number {
    throw new Error("not implemented");
  }
  after(_ms: number, _run: () => void): Disposable {
    throw new Error("not implemented");
  }
  advance(_ms: number): void {
    throw new Error("not implemented");
  }
  pending(): number {
    throw new Error("not implemented");
  }
}
