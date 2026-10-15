import { execFile } from "node:child_process";
import { appendFileSync, chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { adapter, type Options } from "../lib/adapter.ts";
import type { Ctx, Message, PiApi, SendOptions, ToolDefinition } from "../lib/pi.ts";

/** A fake `cpd2` on PATH and a fake pi, so the adapter is tested through its real boundaries (child processes, pi's API). */

const FAKE_CPD2 = `#!/usr/bin/env node
const fs = require("fs");
const path = require("path");
const dir = process.env.FAKE_CPD2_DIR;
const argv = process.argv.slice(2);
const log = (o) => fs.appendFileSync(path.join(dir, "calls.jsonl"), JSON.stringify({ ...o, t: Date.now() }) + "\\n");
const read = (f) => { try { return fs.readFileSync(path.join(dir, f), "utf8"); } catch { return undefined; } };
const flag = (n) => { const i = argv.indexOf(n); return i >= 0 ? argv[i + 1] : undefined; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function follow() {
  const since = Number(flag("--since"));
  fs.appendFileSync(path.join(dir, "follower.pids"), process.pid + "\\n");
  process.on("SIGTERM", () => { fs.writeFileSync(path.join(dir, "killed-" + process.pid), "1"); process.exit(143); });
  let done = 0;
  for (;;) {
    const lines = (read("events.jsonl") || "").split("\\n").filter(Boolean);
    while (done < lines.length) {
      const line = lines[done++];
      let id;
      try { id = JSON.parse(line).id; } catch {}
      if (Number.isSafeInteger(id) && id <= since) continue;
      process.stdout.write(line + "\\n");
    }
    const failure = read("follow-failure.json");
    if (failure !== undefined) {
      const f = JSON.parse(failure);
      process.stdout.write(f.line + "\\n");
      process.exit(f.exit);
    }
    const exitNext = read("exit-next");
    if (exitNext !== undefined) { fs.rmSync(path.join(dir, "exit-next")); process.exit(Number(exitNext)); }
    await sleep(10);
  }
}

(async () => {
  log({ phase: "start", argv, since: flag("--since") });
  if (argv[0] === "events" && argv[1] === "follow") return follow();
  const key = argv.slice(0, 2).join("-");
  const r = JSON.parse(read("resp/" + key + ".json") || '{"stdout":{"ok":true}}');
  let feedback;
  if (flag("--feedback-file")) feedback = fs.readFileSync(flag("--feedback-file"), "utf8");
  if (r.delay) await sleep(r.delay);
  log({ phase: "end", argv, feedback });
  process.stdout.write(typeof r.stdout === "string" ? r.stdout : JSON.stringify(r.stdout));
  process.exit(r.exit || 0);
})();
`;

export const ID = "596d007f-6775-4134-b77a-b356cc523842";
export const OTHER = "11111111-2222-4333-8444-555555555555";

export type Call = { phase: "start" | "end"; argv: string[]; since?: string; feedback?: string; t: number };

export class Fixture {
  readonly dir = mkdtempSync(join(tmpdir(), "pi-v2-test-"));
  readonly cpdHome = join(this.dir, "cpd-home");
  readonly pi: FakePi;
  private readonly saved: Record<string, string | undefined> = {};

  constructor() {
    mkdirSync(join(this.dir, "bin"));
    mkdirSync(join(this.dir, "resp"));
    mkdirSync(this.cpdHome);
    writeFileSync(join(this.dir, "bin", "cpd2"), FAKE_CPD2);
    chmodSync(join(this.dir, "bin", "cpd2"), 0o755);
    for (const k of ["PATH", "FAKE_CPD2_DIR", "CPD_SESSION", "CPD_PLAN", "CPD_HOME"]) this.saved[k] = process.env[k];
    process.env.PATH = `${join(this.dir, "bin")}:${process.env.PATH}`;
    process.env.FAKE_CPD2_DIR = this.dir;
    process.env.CPD_SESSION = ID;
    process.env.CPD_HOME = this.cpdHome;
    delete process.env.CPD_PLAN;
    this.pi = new FakePi();
    this.respond("session-show", sessionShow());
  }

  /** What `cpd2 <a> <b>` prints (an object, or a raw string) and exits with. */
  respond(key: string, stdout: unknown, extra: { exit?: number; delay?: number } = {}): void {
    writeFileSync(join(this.dir, "resp", `${key}.json`), JSON.stringify({ stdout, ...extra }));
  }

  /** Appends Event lines (objects or raw strings) to what the fake follower prints. */
  emit(...lines: unknown[]): void {
    for (const l of lines) appendFileSync(join(this.dir, "events.jsonl"), (typeof l === "string" ? l : JSON.stringify(l)) + "\n");
  }

  file(name: string, content: string): void {
    writeFileSync(join(this.dir, name), content);
  }

  remove(name: string): void {
    rmSync(join(this.dir, name), { force: true });
  }

  planFile(plan: string, review: string, content: string): string {
    const p = join(this.cpdHome, "run", plan, `plan-${review}.md`);
    mkdirSync(join(this.cpdHome, "run", plan), { recursive: true });
    writeFileSync(p, content);
    return p;
  }

  calls(): Call[] {
    const f = join(this.dir, "calls.jsonl");
    return existsSync(f) ? readFileSync(f, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l)) : [];
  }

  /** argv of the calls that started, in order, without `events follow`. */
  argvs(): string[][] {
    return this.calls().filter((c) => c.phase === "start" && c.argv[0] !== "events").map((c) => c.argv);
  }

  follows(): Call[] {
    return this.calls().filter((c) => c.phase === "start" && c.argv[0] === "events");
  }

  /** `--since` of every follower start. */
  sinces(): number[] {
    return this.follows().map((c) => Number(c.since));
  }

  pids(): number[] {
    const f = join(this.dir, "follower.pids");
    return existsSync(f) ? readFileSync(f, "utf8").split("\n").filter(Boolean).map(Number) : [];
  }

  killed(pid: number): boolean {
    return existsSync(join(this.dir, `killed-${pid}`));
  }

  /** Start the adapter under test with fast timing. */
  load(options: Options = {}): void {
    adapter(this.pi, { timing: { restartInitialMs: 30, restartMaxMs: 120, requestTimeoutMs: 300, pollMs: 30, shutdownWaitMs: 200 }, ...options });
  }

  async close(): Promise<void> {
    await this.pi.fire("session_shutdown").catch(() => {});
    for (const [k, v] of Object.entries(this.saved)) {
      if (v === undefined) delete process.env[k];
      else process.env[k] = v;
    }
    rmSync(this.dir, { recursive: true, force: true });
  }
}

export const sessionShow = (over: Record<string, unknown> = {}) => ({
  ok: true,
  session: { id: ID, role: "lead", context: "ctx", plan: "p1", attempt: null, name: "the lead", state: "stopped", plan_lead: true, grounded_lead: false, registered_event: 100, ...over },
  open_escalations: [],
});

export const planShow = (over: Record<string, unknown> = {}) => ({ ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null, ...over });

export const ev = (id: number, kind: string, over: Record<string, unknown> = {}) => ({
  id, at: "2026-10-14T21:41:02.114Z", kind, context: "ctx", plan: "p1", job: null, attempt: null, session: null, text: `text of ${kind} ${id}`, data: {}, ...over,
});

export type Sent = { message: Message; options: SendOptions };

export class FakePi implements PiApi {
  private handlers = new Map<string, ((event: any, ctx: Ctx) => unknown)[]>();
  private listeners = new Map<string, ((data: unknown) => void)[]>();
  readonly sent: Sent[] = [];
  /** The session's entries, as pi's `getEntries()` returns them: messages and custom entries appended so far. */
  entries: unknown[] = [];
  readonly tools: ToolDefinition[] = [];
  readonly notices: string[] = [];
  readonly emitted: { channel: string; data: any }[] = [];
  sessionName: string | undefined;
  idle = true;
  mode = "tui";
  sessionId = ID;

  readonly events = {
    emit: (channel: string, data: unknown) => {
      this.emitted.push({ channel, data });
      for (const l of [...(this.listeners.get(channel) ?? [])]) l(data);
    },
    on: (channel: string, handler: (data: unknown) => void) => {
      this.listeners.set(channel, [...(this.listeners.get(channel) ?? []), handler]);
      return () => this.listeners.set(channel, (this.listeners.get(channel) ?? []).filter((h) => h !== handler));
    },
  };

  on(event: string, handler: (event: any, ctx: Ctx) => unknown): () => void {
    this.handlers.set(event, [...(this.handlers.get(event) ?? []), handler]);
    return () => {};
  }

  exec(command: string, args: string[]): Promise<{ stdout: string; stderr: string; code: number }> {
    return new Promise((resolve) => {
      execFile(command, args, { encoding: "utf8" }, (err, stdout, stderr) => {
        resolve({ stdout, stderr, code: err ? (typeof err.code === "number" ? err.code : 1) : 0 });
      });
    });
  }

  sendMessage(message: Message, options: SendOptions): void {
    this.sent.push({ message, options });
    this.entries.push({ type: "custom_message", customType: message.customType, content: message.content, display: message.display, details: message.details });
  }

  appendEntry(customType: string, data: unknown): void {
    this.entries.push({ type: "custom", customType, data });
  }

  setSessionName(name: string): void {
    this.sessionName = name;
  }

  getSessionName(): string | undefined {
    return this.sessionName;
  }

  registerTool(tool: ToolDefinition): void {
    this.tools.push(tool);
  }

  ctx(): Ctx {
    return {
      mode: this.mode,
      ui: { notify: (m) => void this.notices.push(m) },
      sessionManager: { getSessionId: () => this.sessionId, getEntries: () => this.entries },
      isIdle: () => this.idle,
    };
  }

  async fire(event: string, payload: Record<string, unknown> = {}): Promise<void> {
    for (const h of this.handlers.get(event) ?? []) await h({ type: event, ...payload }, this.ctx());
  }

  /** `cpd` messages delivered (not the adapter's own errors). */
  delivered(): Sent[] {
    return this.sent.filter((s) => s.message.customType === "cpd" && (s.message.details as any)?.delivered_to !== undefined);
  }

  errors(): Sent[] {
    return this.sent.filter((s) => s.message.customType === "cpd" && (s.message.details as any)?.kind === "error");
  }
}

export async function waitFor<T>(what: string, fn: () => T | undefined | null | false, ms = 3000): Promise<T> {
  const deadline = Date.now() + ms;
  for (;;) {
    const v = fn();
    if (v) return v;
    if (Date.now() > deadline) throw new Error(`timed out after ${ms} ms waiting for ${what}`);
    await sleep(10);
  }
}

/** Wait long enough for something that must NOT happen. */
export const settle = (ms = 150) => sleep(ms);

/** `session state` argvs, without the session id, as `idle` or `blocked:<label>`. */
export const stateCalls = (f: Fixture): string[] =>
  f.argvs().filter((a) => a[0] === "session" && a[1] === "state").map((a) => (a[2] === "blocked" ? `blocked:${a[a.indexOf("--label") + 1]}` : a[2]!));
