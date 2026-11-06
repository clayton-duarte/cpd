import { describe, expect, it } from "vitest";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { WebError } from "../../protocol/domain.ts";
import { createCockpitStream, type StreamChild, type StreamSnapshot } from "./stream.ts";

class FakeChild implements StreamChild {
  writes: string[] = [];
  ended = false;
  killed = false;
  log: string[] = [];
  private lineCb: (l: string) => void = () => undefined;
  private exitCb: (i: { code: number | null; stderr: string }) => void = () => undefined;
  write(line: string): void { this.writes.push(line); }
  endInput(): void { this.ended = true; this.log.push("endInput"); }
  kill(): void { this.killed = true; this.log.push("kill"); }
  onLine(cb: (l: string) => void): void { this.lineCb = cb; }
  onExit(cb: (i: { code: number | null; stderr: string }) => void): void { this.exitCb = cb; }
  feed(frame: unknown): void { this.lineCb(typeof frame === "string" ? frame : JSON.stringify(frame)); }
  exit(code: number | null = 1): void { this.exitCb({ code, stderr: "" }); }
}

const reply = { ok: true, events_cursor: 1 };
const panes = { context: reply, plan: reply, session: reply, inbox: reply };
const hello = { t: "hello", protocol: 1 };

function setup() {
  const scheduler = new FakeScheduler();
  const children: FakeChild[] = [];
  const argsSeen: string[][] = [];
  const stream = createCockpitStream({ spawn: (args) => { argsSeen.push(args); const c = new FakeChild(); children.push(c); return c; }, scheduler });
  const snapshots: StreamSnapshot[] = [];
  const errors: Error[] = [];
  stream.onSnapshot((s) => snapshots.push(s));
  stream.onError((e) => errors.push(e));
  return { stream, scheduler, children, argsSeen, snapshots, errors };
}

describe("cockpit stream", () => {
  it("parses a hello then a snapshot frame into one ready snapshot", () => {
    const { stream, children, argsSeen, snapshots, errors } = setup();
    stream.start();
    expect(argsSeen).toEqual([["cockpit", "serve"]]);
    expect(stream.ready()).toBe(false);
    children[0]!.feed(hello);
    expect(stream.ready()).toBe(true);
    children[0]!.feed({ t: "snapshot", seq: 1, panes });
    expect(snapshots).toEqual([{ seq: 1, panes }]);
    expect(errors).toEqual([]);
  });

  it("ignores a blank line and reports a malformed line as an error without stopping", () => {
    const { stream, children, snapshots, errors } = setup();
    stream.start();
    children[0]!.feed(hello);
    children[0]!.feed("");
    children[0]!.feed("   ");
    expect(errors).toEqual([]);
    children[0]!.feed("not json");
    children[0]!.feed({ t: "bogus" });
    expect(errors).toHaveLength(2);
    for (const e of errors) {
      expect(e).toBeInstanceOf(WebError);
      expect((e as WebError).code).toBe("cpd_malformed_json");
    }
    expect(children[0]!.killed).toBe(false);
    expect(stream.ready()).toBe(true);
    children[0]!.feed({ t: "snapshot", seq: 1, panes });
    expect(snapshots).toHaveLength(1);
  });

  it("plan show request resolves with the matching plan show frame", async () => {
    const { stream, children } = setup();
    stream.start();
    children[0]!.feed(hello);
    const first = stream.planShow("p1");
    const second = stream.planShow("p2");
    expect(children[0]!.writes).toHaveLength(2);
    const [r1, r2] = children[0]!.writes.map((w) => JSON.parse(w) as { t: string; id: string; plan: string });
    expect(r1).toMatchObject({ t: "plan_show", plan: "p1" });
    expect(r2).toMatchObject({ t: "plan_show", plan: "p2" });
    expect(typeof r1!.id).toBe("string");
    expect(r1!.id).not.toBe(r2!.id);
    children[0]!.feed({ t: "plan_show", id: r2!.id, value: { which: "two" } });
    children[0]!.feed({ t: "plan_show", id: r1!.id, value: { which: "one" } });
    await expect(first).resolves.toEqual({ which: "one" });
    await expect(second).resolves.toEqual({ which: "two" });
  });

  it("an error frame with a request id rejects that request", async () => {
    const { stream, children, errors } = setup();
    stream.start();
    children[0]!.feed(hello);
    const p = stream.planShow("nope");
    const assertion = expect(p).rejects.toMatchObject({ code: "cpd_failed", details: { cpdCode: "plan_not_found" } });
    const { id } = JSON.parse(children[0]!.writes[0]!) as { id: string };
    children[0]!.feed({ t: "error", id, error: { code: "plan_not_found", message: "no such plan" } });
    await assertion;
    expect(errors).toEqual([]);
  });

  it("restarts the child with doubling backoff capped at ten seconds", () => {
    const { stream, scheduler, children } = setup();
    stream.start();
    expect(children).toHaveLength(1);
    let n = 1;
    for (const wait of [500, 1000, 2000, 4000, 8000, 10000, 10000]) {
      children[n - 1]!.exit(1);
      scheduler.advance(wait - 1);
      expect(children).toHaveLength(n);
      scheduler.advance(1);
      n++;
      expect(children).toHaveLength(n);
    }
  });

  it("a hello resets the backoff", () => {
    const { stream, scheduler, children } = setup();
    stream.start();
    children[0]!.exit(1);
    scheduler.advance(500);
    children[1]!.exit(1);
    scheduler.advance(999);
    expect(children).toHaveLength(2);
    scheduler.advance(1);
    expect(children).toHaveLength(3);
    children[2]!.feed(hello);
    children[2]!.exit(1);
    scheduler.advance(499);
    expect(children).toHaveLength(3);
    scheduler.advance(1);
    expect(children).toHaveLength(4);
  });

  it("stop ends stdin and terminates the child and cancels the restart", () => {
    const { stream, scheduler, children, errors, snapshots } = setup();
    stream.start();
    children[0]!.feed(hello);
    stream.stop();
    expect(children[0]!.log).toEqual(["endInput", "kill"]);
    expect(stream.ready()).toBe(false);
    children[0]!.exit(0);
    children[0]!.feed({ t: "snapshot", seq: 1, panes });
    scheduler.advance(100_000);
    expect(children).toHaveLength(1);
    expect(errors).toEqual([]);
    expect(snapshots).toEqual([]);
    expect(scheduler.pending()).toBe(0);

    const other = setup();
    other.stream.start();
    other.children[0]!.exit(1);
    expect(other.scheduler.pending()).toBe(1);
    other.stream.stop();
    expect(other.scheduler.pending()).toBe(0);
    other.scheduler.advance(100_000);
    expect(other.children).toHaveLength(1);
  });

  it("is not ready between a child exit and the next hello", () => {
    const { stream, scheduler, children } = setup();
    stream.start();
    children[0]!.feed(hello);
    expect(stream.ready()).toBe(true);
    children[0]!.exit(1);
    expect(stream.ready()).toBe(false);
    scheduler.advance(500);
    expect(children).toHaveLength(2);
    expect(stream.ready()).toBe(false);
    children[1]!.feed(hello);
    expect(stream.ready()).toBe(true);
  });

  it("a second hello resolves a pending planShow down and keeps the stream ready without onError", async () => {
    const { stream, children, errors } = setup();
    stream.start();
    children[0]!.feed(hello);
    const pending = stream.planShow("p1");
    expect(children[0]!.writes).toHaveLength(1);
    children[0]!.feed(hello);
    expect(await Promise.race([pending, Promise.resolve("pending")])).toBe("down");
    expect(stream.ready()).toBe(true);
    expect(errors).toEqual([]);
  });

  const tooNew = { t: "error", id: null, error: { code: "store_too_new", message: "store is newer than this cpd" } };

  it("store_too_new error frame restarts the child at once and reports no error", () => {
    const { stream, scheduler, children, errors } = setup();
    stream.start();
    children[0]!.feed(hello);
    children[0]!.feed(tooNew);
    expect(children[0]!.killed).toBe(true);
    expect(children).toHaveLength(2);
    expect(scheduler.pending()).toBe(0);
    children[0]!.exit(1);
    expect(children).toHaveLength(2);
    expect(errors).toEqual([]);
  });

  it("store_too_new plan_show rejection restarts the child and resolves the request down", async () => {
    const { stream, scheduler, children, errors } = setup();
    stream.start();
    children[0]!.feed(hello);
    const outcome = stream.planShow("p1").catch(() => "rejected");
    const { id } = JSON.parse(children[0]!.writes[0]!) as { id: string };
    children[0]!.feed({ t: "error", id, error: { code: "store_too_new", message: "store is newer than this cpd" } });
    expect(children[0]!.killed).toBe(true);
    expect(children).toHaveLength(2);
    expect(scheduler.pending()).toBe(0);
    expect(await outcome).toBe("down");
    expect(errors).toEqual([]);
  });

  it("a second store_too_new before hello after a heal restart is reported", () => {
    const { stream, scheduler, children, errors } = setup();
    stream.start();
    children[0]!.feed(hello);
    children[0]!.feed(tooNew);
    expect(children).toHaveLength(2);
    children[1]!.feed(tooNew);
    expect(errors).toHaveLength(1);
    expect(errors[0]).toBeInstanceOf(WebError);
    expect((errors[0] as WebError).code).toBe("cpd_failed");
    expect((errors[0] as WebError).details?.["cpdCode"]).toBe("store_too_new");
    children[1]!.exit(1);
    expect(errors).toHaveLength(1);
    scheduler.advance(499);
    expect(children).toHaveLength(2);
    scheduler.advance(1);
    expect(children).toHaveLength(3);
  });
});
