import { expect, it } from "vitest";
import { FakeScheduler } from "./scheduler.ts";

it("advance fires due timers in order", () => {
  const s = new FakeScheduler();
  const log: string[] = [];
  s.after(20, () => log.push("b"));
  s.after(10, () => log.push("a"));
  s.after(20, () => log.push("c"));
  s.advance(25);
  expect(log).toEqual(["a", "b", "c"]);
});

it("a cancelled timer never fires", () => {
  const s = new FakeScheduler();
  let fired = 0;
  const d = s.after(10, () => fired++);
  d.dispose();
  d.dispose();
  s.advance(100);
  expect(fired).toBe(0);
  expect(s.pending()).toBe(0);
});

it("advance does not fire timers beyond the delta", () => {
  const s = new FakeScheduler();
  let fired = 0;
  const start = s.now();
  s.after(50, () => fired++);
  s.advance(49);
  expect(fired).toBe(0);
  expect(s.now()).toBe(start + 49);
  s.advance(1);
  expect(fired).toBe(1);
});

it("pending timers are reported so tests can assert none leak", () => {
  const s = new FakeScheduler();
  expect(s.pending()).toBe(0);
  s.after(10, () => {});
  s.after(20, () => {});
  expect(s.pending()).toBe(2);
  s.advance(10);
  expect(s.pending()).toBe(1);
  s.advance(10);
  expect(s.pending()).toBe(0);
});
