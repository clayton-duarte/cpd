import { type ChildProcess, type spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import { appendFileSync, chmodSync, type FSWatcher, watch, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PassThrough } from "node:stream";
import { adapter, type Options } from "../lib/adapter.ts";
import { usageError } from "./usage.ts";
import { answers, ID, policyShow, sessionShow, shown } from "./answers.ts";
import type { CommandCtx, Ctx, ExecOptions, ExecResult, Message, PiApi, SendOptions, ToolDefinition } from "../lib/pi.ts";

/**
 * A fake `cpd` and a fake pi, in this process, so the adapter is tested through its real boundaries (pi's `exec`, the `spawn` of its follower)
 * and no node child runs per call (a process start-up is hundreds of ms under load, against the adapter's 200-300 ms limits).
 * What waits is event-driven: every change a test can wait for is announced with {@link notify}, and {@link waitFor} looks again then.
 */

const watchers = new Set<() => void>();

/** Announces that something a `waitFor` condition reads may have changed. */
export function notify(): void {
  for (const w of [...watchers]) w();
}

/** The timers of the fake's own delays and timeouts (`respond(..., { delay })`, `ExecOptions.timeout`): real ones, until a test passes others. */
export type Timers = { setTimeout(run: () => void, ms: number): unknown; clearTimeout(handle: unknown): void };
const realTimers: Timers = { setTimeout: (run, ms) => setTimeout(run, ms), clearTimeout: (handle) => clearTimeout(handle as NodeJS.Timeout) };

const flagOf = (argv: string[], name: string): string | undefined => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};

type Answer = { stdout: unknown; exit?: number; delay?: number };

/** The child process of a fake `cpd events follow`: `stdout`, `kill()`, `pid` and the `exit` and `close` events of a real one. */
class FakeFollower extends EventEmitter {
  readonly stdout = new PassThrough();
  readonly stderr = new PassThrough();
  /** Lines of `FakeCpd.lines` printed so far. */
  private done = 0;
  private finished = false;

  readonly pid: number;
  private readonly since: number;
  private readonly cpd: FakeCpd;

  constructor(pid: number, since: number, cpd: FakeCpd) {
    super();
    this.pid = pid;
    this.since = since;
    this.cpd = cpd;
  }

  /** Prints what was emitted since the last time, then ends as `failFollower` or `exitNext` say. Every line emitted before an exit is printed before it. */
  pump(): void {
    if (this.finished) return;
    const cpd = this.cpd;
    for (; this.done < cpd.lines.length; this.done++) {
      const line = cpd.lines[this.done]!;
      let id: unknown;
      try {
        id = JSON.parse(line).id;
      } catch {
        // not JSON: no id
      }
      if (Number.isSafeInteger(id) && (id as number) <= this.since) continue;
      // `idLessOnce`: an id-less line is printed by the first follower to reach it only (a new binary that no longer fails)
      if (id === undefined && cpd.idLessOnceSet) {
        if (cpd.idLessPrinted.has(line)) continue;
        cpd.idLessPrinted.add(line);
      }
      this.stdout.write(line + "\n");
    }
    if (cpd.failure !== undefined) {
      this.stdout.write(cpd.failure.line + "\n");
      this.finish(cpd.failure.exit);
    } else if (cpd.exitNextCode !== undefined) {
      const code = cpd.exitNextCode;
      cpd.exitNextCode = undefined;
      this.finish(code);
    }
  }

  /** Like a SIGTERM the fake's handler answers with exit 143. */
  kill(_signal?: string): boolean {
    if (this.finished) return false;
    this.cpd.killedPids.add(this.pid);
    this.finish(143);
    return true;
  }

  private finish(code: number): void {
    this.finished = true;
    this.stdout.end();
    this.stderr.end();
    // the `close` of a child follows the end of its stdout, once the reader has everything
    this.stdout.once("end", () => {
      this.emit("exit", code, null);
      this.emit("close", code, null);
      this.cpd.followersRunning.delete(this);
      notify();
    });
    notify();
  }
}

export type Call = { phase: "start" | "end"; argv: string[]; since?: string; feedback?: string; t: number };

/** What the fake `cpd` knows and has seen: the answers, the Events the followers print, the calls. The Fixture owns one; a second `FakePi` runs against the same one. */
export class FakeCpd {
  timers: Timers = realTimers;
  readonly responses = new Map<string, Answer>();
  /** The Event lines emitted so far (objects as JSON). */
  readonly lines: string[] = [];
  readonly callLog: Call[] = [];
  /** Every follower started, by pid; `killedPids` are the ones `kill()` ended. */
  readonly followers: FakeFollower[] = [];
  readonly followersRunning = new Set<FakeFollower>();
  readonly killedPids = new Set<number>();
  /** The exit code the next follower to reach it ends with, once. */
  exitNextCode: number | undefined;
  /** Every follower prints `line` and ends with `exit`, until cleared. */
  failure: { line: string; exit: number } | undefined;
  idLessOnceSet = false;
  readonly idLessPrinted = new Set<string>();
  /** Calls in flight with a delay: what ends each as killed. */
  private readonly pending = new Set<() => void>();
  private tick = 0;
  private pids = 0;

  private log(call: Omit<Call, "t">): void {
    this.callLog.push({ ...call, t: ++this.tick });
    notify();
  }

  /** The answer of an argv: `<a>-<b>@<kind>@machine` for a call with no `--context`, `<a>-<b>@<kind>`, `<a>-<b>-<c>`, `<a>-<b>`, then `{"ok":true}`. */
  private answer(argv: string[]): Answer {
    const key = argv.slice(0, 2).join("-");
    const kind = flagOf(argv, "--kind");
    const machine = flagOf(argv, "--context") === undefined && kind !== undefined ? this.responses.get(`${key}@${kind}@machine`) : undefined;
    return machine ?? (kind === undefined ? undefined : this.responses.get(`${key}@${kind}`)) ?? this.responses.get(`${key}-${argv[2]}`) ?? this.responses.get(key) ?? { stdout: { ok: true } };
  }

  /** `cpd <argv>` as pi's `exec` runs it: `signal` and `timeout` end it as `killed`, like pi's. */
  call(argv: string[], options: ExecOptions): Promise<ExecResult> {
    const killed: ExecResult = { stdout: "", stderr: "", code: 0, killed: true };
    const { signal, timeout } = options;
    if (signal?.aborted) return Promise.resolve(killed);
    this.log({ phase: "start", argv, since: flagOf(argv, "--since") });
    const refused = usageError(argv);
    const r: Answer = refused === undefined ? this.answer(argv) : { stdout: { ok: false, error: { code: "usage", message: refused, details: {} } }, exit: 2 };
    let feedback: string | undefined;
    const file = flagOf(argv, "--feedback-file");
    try {
      if (file !== undefined) feedback = readFileSync(file, "utf8");
    } catch (e) {
      return new Promise((resolve) => setImmediate(resolve, { stdout: "", stderr: String(e), code: 1, killed: false }));
    }
    return new Promise((resolve) => {
      let delayTimer: unknown;
      let limit: unknown;
      const conclude = (result: ExecResult) => {
        if (delayTimer !== undefined) this.timers.clearTimeout(delayTimer);
        if (limit !== undefined) this.timers.clearTimeout(limit);
        signal?.removeEventListener("abort", abort);
        this.pending.delete(abort);
        resolve(result);
      };
      const abort = () => conclude(killed);
      const end = () => {
        this.log({ phase: "end", argv, feedback });
        conclude({ stdout: typeof r.stdout === "string" ? r.stdout : JSON.stringify(r.stdout), stderr: "", code: r.exit ?? 0, killed: false });
      };
      signal?.addEventListener("abort", abort, { once: true });
      if (r.delay) {
        // stays in flight until the delay is over, the timeout, the signal or `cancelPending`
        this.pending.add(abort);
        delayTimer = this.timers.setTimeout(end, r.delay);
        if (timeout) limit = this.timers.setTimeout(abort, timeout);
      } else {
        setImmediate(end);
      }
    });
  }

  /** `spawn("cpd", ["events", "follow", …])`: a fake follower that prints the Events emitted so far with an id above `--since`, then each new one at once. */
  spawn = ((_command: string, argv: string[]) => {
    const since = Number(flagOf(argv, "--since"));
    const follower = new FakeFollower(++this.pids, since, this);
    this.followers.push(follower);
    this.followersRunning.add(follower);
    this.log({ phase: "start", argv, since: flagOf(argv, "--since") });
    follower.pump();
    return follower as unknown as ChildProcess;
  }) as unknown as typeof spawn;

  /** Every follower looks again at what it has to print and whether it ends. */
  pump(): void {
    for (const f of [...this.followersRunning]) f.pump();
    notify();
  }

  /** Ends every call still in flight with a delay, as killed. */
  cancelPending(): void {
    for (const abort of [...this.pending]) abort();
  }
}

/** The fake `cpd` a `FakePi` made without one runs against: the one of the Fixture alive. */
let active: FakeCpd | undefined;

/** Stands in the Fixture's directory for the installed `cpd`: the adapter stats the first `cpd` on `PATH` (its identity, §15.2 step 0) and never runs it, the fake runs in process. */
const CPD_FILE = `#!/bin/sh\n# stands for the installed cpd: the adapter stats this file, the tests' fake cpd answers in process (pi/test/fake.ts)\nexit 1\n`;

/**
 * Stands for pi in the pre-validation (`pi --no-extensions --no-session -e <dir> --mode rpc`; the real one is pi's, not a dependency of this repo): loads
 * `<dir>/extension.ts` natively, which node does for `.ts`, and exits 0, or 1 with pi's message on stderr when the load throws or the default export is not a function.
 * Records its environment in `validate-calls.jsonl` (the scratch agent directory must exist while it runs).
 */
const FAKE_PI = `#!/usr/bin/env node
const fs = require("fs");
const path = require("path");
const argv = process.argv.slice(2);
const dir = argv[argv.indexOf("-e") + 1];
const entry = path.join(dir, "extension.ts");
fs.appendFileSync(path.join(process.env.FAKE_PI_DIR, "validate-calls.jsonl"), JSON.stringify({ argv, cwd: process.cwd(), agentDir: process.env.PI_CODING_AGENT_DIR, offline: process.env.PI_OFFLINE, agentDirExists: fs.existsSync(process.env.PI_CODING_AGENT_DIR) }) + "\\n");
import(entry).then((m) => {
  if (typeof m.default !== "function") throw new Error("Extension does not export a valid factory function: " + entry);
  process.exit(0);
}).catch((e) => {
  console.error('Error: Failed to load extension "' + entry + '": ' + (e instanceof Error ? e.message : String(e)) + "\\nHint: Start without extensions using \\"pi -ne\\".");
  process.exit(1);
});
`;

/** Defaults of the key surfaces: a `session show` carries `key` (the name of a keyed role, null for a fork: `forked_from` set), a taken post carries `from_name` (null for the User). A test that sets the field keeps it. */
function withKeyFields(key: string, stdout: unknown): unknown {
  const o = stdout as Record<string, any> | null;
  if (key === "session-show" && o && typeof o === "object" && o.session && typeof o.session === "object" && !("key" in o.session)) {
    const s = o.session;
    return { ...o, session: { ...s, key: s.forked_from ? null : (s.name ?? null) } };
  }
  if (key === "board-take" && o && Array.isArray(o.taken)) {
    return { ...o, taken: o.taken.map((p: any) => ("from_name" in p ? p : { ...p, from_name: p.from_session === null ? null : `name-of-${p.from_session}` })) };
  }
  return stdout;
}

export { ID, policyShow, sessionShow, shown };
export const OTHER = "11111111-2222-4333-8444-555555555555";

export type Report = { argv: string[]; what?: string; code?: string; message?: string; session?: string; details?: Record<string, unknown> };

export class Fixture {
  /** The fake `cpd` (answers, Events, calls, followers) this Fixture and every `FakePi` made without one run against. */
  readonly cpd = new FakeCpd();
  readonly dir = mkdtempSync(join(tmpdir(), "pi-test-"));
  readonly cpdHome = join(this.dir, "cpd-home");
  /** Stands for the adapter's installed directory (`options.reload.dir`). */
  readonly adapterDir = join(this.dir, "adapter");
  readonly agentDir = join(this.dir, "agent");
  readonly pi: FakePi;
  private readonly saved: Record<string, string | undefined> = {};
  /** What the adapter writes under `CPD_HOME` (its log: `adapterLog()`) is announced by the file system, so a `waitFor` on it looks again then. */
  private readonly watcher: FSWatcher;

  constructor() {
    mkdirSync(join(this.dir, "bin"));
    mkdirSync(join(this.dir, "resp"));
    mkdirSync(this.cpdHome);
    this.watcher = watch(this.cpdHome, { recursive: true }, () => notify());
    mkdirSync(this.adapterDir);
    for (const skill of ["cpd-lead", "cpd-manager"]) {
      mkdirSync(join(this.adapterDir, "skills", skill), { recursive: true });
      writeFileSync(join(this.adapterDir, "skills", skill, "SKILL.md"), `the ${skill} skill\n`); // a readable skills directory (commands.md §15.23)
    }
    writeFileSync(join(this.dir, "bin", "pi"), FAKE_PI);
    chmodSync(join(this.dir, "bin", "pi"), 0o755);
    writeFileSync(join(this.dir, "bin", "cpd"), CPD_FILE);
    chmodSync(join(this.dir, "bin", "cpd"), 0o755);
    for (const k of ["PATH", "FAKE_PI_DIR", "CPD_SESSION", "CPD_PLAN", "CPD_HOME", "PI_CODING_AGENT_DIR"]) this.saved[k] = process.env[k];
    process.env.PATH = `${join(this.dir, "bin")}:${process.env.PATH}`;
    process.env.FAKE_PI_DIR = this.dir;
    process.env.CPD_SESSION = ID;
    process.env.CPD_HOME = this.cpdHome;
    process.env.PI_CODING_AGENT_DIR = this.agentDir; // pi's agent directory: where the adapter reads `compact-tools.json` (commands.md §15.9)
    delete process.env.CPD_PLAN;
    active = this.cpd;
    this.pi = new FakePi(this.cpd);
    for (const [key, answer] of Object.entries(answers())) this.respond(key, answer); // the same answers as pi-check's fake (answers.ts)
    this.respond("board-take", { ok: true, session: ID, taken: [] }); // a Lead or Manager takes its board posts at every turn start (commands.md §15.7)
    this.respond("events-list", { ok: true, events: [], next: 0 });
    this.respond("session-switch-failed", { ok: true }); // the adapter records a failed model switch (commands.md §10.16)
    this.respond("session-switched", { ok: true }); // and a successful one
    this.respond("session-report", { ok: true, event: "4823" }); // a Lead's adapter reports its errors to the Manager (commands.md §10.18)
  }

  /** A new install of the fake `cpd`: the same program in a file of another size and mtime. */
  reinstall(): void {
    const file = join(this.dir, "bin", "cpd");
    appendFileSync(file, `// install ${++this.installs}\n`);
    const later = new Date(Date.now() + 1000 * this.installs);
    utimesSync(file, later, later);
  }
  private installs = 0;

  /** What `cpd <a> <b>` prints (an object, or a raw string) and exits with, and how long (`delay`, ms of the fake's timers) it stays in flight. Key `<a>-<b>`; `<a>-<b>-<c>` answers `cpd <a> <b> <c>` first; `<a>-<b>@<kind>` answers a call whose first `--kind` is `<kind>`, before both. */
  respond(key: string, stdout: unknown, extra: { exit?: number; delay?: number } = {}): void {
    this.cpd.responses.set(key, { stdout: withKeyFields(key, stdout), ...extra });
  }

  /** Appends Event lines (objects or raw strings) to what the fake follower prints. */
  emit(...lines: unknown[]): void {
    for (const l of lines) {
      this.cpd.lines.push(typeof l === "string" ? l : JSON.stringify(l));
      const id = (l as { id?: unknown }).id;
      if (typeof id === "number") this.lastId = Math.max(this.lastId, id);
    }
    this.cpd.pump();
  }

  /** The next follower to look ends with `code`, after printing every line emitted so far; once. */
  exitNext(code: number): void {
    this.cpd.exitNextCode = code;
    this.cpd.pump();
  }

  /** Every follower prints `line` (a failure envelope) and ends with `exit`, until `clearFollowerFailure`. */
  failFollower(line: string, exit: number): void {
    this.cpd.failure = { line, exit };
    this.cpd.pump();
  }

  clearFollowerFailure(): void {
    this.cpd.failure = undefined;
  }

  /** An id-less line is printed by the first follower to reach it only: the new `cpd` no longer fails, the restarted follower does not print the line again. */
  idLessOnce(): void {
    this.cpd.idLessOnceSet = true;
  }

  /** The highest Event id emitted so far (a `drain` marker follows it, and a restart's cursor passes it). */
  private lastId = 0;

  /** Writes a file of the Fixture's directory (the adapter reads `agent/compact-tools.json`), atomically (write, then rename). */
  file(name: string, content: string): void {
    const moved: Record<string, string> = { "exit-next": "exitNext", "follow-failure.json": "failFollower", "id-less-once": "idLessOnce" };
    if (name in moved) throw new Error(`the fake cpd reads no file \`${name}\` any more: use f.${moved[name]}()`);
    const tmp = join(this.dir, `.tmp-${++this.writes}`);
    writeFileSync(tmp, content);
    renameSync(tmp, join(this.dir, name));
  }
  private writes = 0;

  remove(name: string): void {
    rmSync(join(this.dir, name), { force: true });
  }

  summaryFile(plan: string, review: string, content: string): string {
    const p = join(this.cpdHome, "run", plan, `summary-${review}.md`);
    mkdirSync(join(this.cpdHome, "run", plan), { recursive: true });
    writeFileSync(p, content);
    return p;
  }

  /** Every call so far, in order; `t` counts the records (a call's `end` is after its `start`). */
  calls(): Call[] {
    return [...this.cpd.callLog];
  }

  /** argv of the calls that started, in order, without `events follow`, `--version`, `policy show` (see `policyCalls`) and the mention-id reads (§15.14: `session list`, `plan list`, `context list`). */
  argvs(): string[][] {
    const mentions = (a: string[]) => a[1] === "list" && ["plan", "context"].includes(a[0]!) || (a[0] === "session" && a[1] === "list" && a.includes("--archived"));
    return this.calls().filter((c) => c.phase === "start" && c.argv[0] !== "events" && c.argv[0] !== "--version" && c.argv[0] !== "policy" && !mentions(c.argv)).map((c) => c.argv);
  }

  /** The `cpd session report` calls that started (commands.md §10.18), in order, each with its argv and its flags read by name (`details` parsed). */
  reports(): Report[] {
    return this.argvs().filter((a) => a[0] === "session" && a[1] === "report").map((argv) => {
      const flag = (n: string) => (argv.includes(n) ? argv[argv.indexOf(n) + 1] : undefined);
      const details = flag("--details");
      return { argv, what: flag("--what"), code: flag("--code"), message: flag("--message"), session: flag("--session"), details: details === undefined ? undefined : JSON.parse(details) };
    });
  }

  /** The lines of the adapter log `$CPD_HOME/run/adapter/<session>.log` (commands.md §15.6 "Log"), each `<time> <line>`; none when the file does not exist. */
  adapterLog(): string[] {
    const path = join(this.cpdHome, "run", "adapter", `${ID}.log`);
    return existsSync(path) ? readFileSync(path, "utf8").split("\n").filter(Boolean) : [];
  }

  /** argv of the `cpd policy …` calls that started. */
  policyCalls(): string[][] {
    return this.calls().filter((c) => c.phase === "start" && c.argv[0] === "policy").map((c) => c.argv);
  }

  /** How often the adapter ran `cpd --version`. */
  versions(): number {
    return this.calls().filter((c) => c.phase === "start" && c.argv[0] === "--version").length;
  }

  follows(): Call[] {
    return this.calls().filter((c) => c.phase === "start" && c.argv[0] === "events" && c.argv[1] === "follow");
  }

  /** Resolves when the adapter started its Event follower. */
  async followerStarted(): Promise<void> {
    await waitFor("the follower", () => this.follows().length > 0);
  }

  /** Resolves when pi was sent `n` user messages. */
  async userMessagesSent(n: number): Promise<void> {
    await waitFor(`${n} user message(s)`, () => this.pi.userMessages.length >= n);
  }

  /** Resolves when the classifier was called `n` times. */
  async classifyCalled(n: number): Promise<void> {
    await waitFor(`${n} classify call(s)`, () => this.pi.classifyCalls.length >= n);
  }

  /** Resolves when the adapter set the blocked state `label`. */
  async stateSet(label: string): Promise<void> {
    await waitFor(`the state ${label}`, () => stateCalls(this).includes(label));
  }

  /** Resolves when `n` notices whose message contains `part` were shown. */
  async noticesContaining(part: string, n: number): Promise<void> {
    await waitFor(`${n} notice(s) containing ${part}`, () => this.pi.noticeLevels.filter((x) => x.message.includes(part)).length >= n);
  }

  /** `--since` of every follower start. */
  sinces(): number[] {
    return this.follows().map((c) => Number(c.since));
  }

  /** The pid of every fake follower started (a counter), and whether `kill()` ended one. */
  pids(): number[] {
    return this.cpd.followers.map((f) => f.pid);
  }

  killed(pid: number): boolean {
    return this.cpd.killedPids.has(pid);
  }

  /** Start the adapter under test with fast timing; into `pi` (default the fixture's) to run a second adapter on the same fake `cpd`. */
  load(options: Options = {}, pi: FakePi = this.pi, adapterFn: typeof adapter = adapter): void {
    const start = options.spawn ?? this.cpd.spawn;
    if (pi !== this.pi) this.others.add(pi);
    // The caller's own `reload` object, completed: the adapter reads it at every poll, so a test can change `timeoutMs` while it runs.
    const reload = options.reload ?? {};
    if (!("dir" in reload)) reload.dir = this.adapterDir; // an explicit `dir: undefined` leaves the adapter's own default path
    reload.intervalMs ??= 20;
    adapterFn(pi, {
      ...options,
      spawn: ((...args: Parameters<typeof spawn>) => pi.track(start(...args))) as typeof spawn,
      timing: { restartInitialMs: 30, restartMaxMs: 120, shutdownWaitMs: 200, callTimeoutMs: 10_000, ...options.timing },
      reload,
    });
  }

  /** Resolves when no `cpd` call is in flight and none can start from what already ran: calls start in the same macrotask as the work that wants them, so one macrotask boundary with none running is quiet. */
  async quiet(): Promise<void> {
    await new Promise((resolve) => setImmediate(resolve));
    await waitFor("no cpd call in flight", () => this.pi.execsRunning.size === 0);
  }

  /** Resolves when every fake follower started so far is gone and its `close` was seen. */
  async exited(): Promise<void> {
    await waitFor("every fake follower to exit", () => this.cpd.followersRunning.size === 0, 10_000);
  }

  /**
   * Resolves when every Event emitted so far was handled, and the calls it made are done. Events are handled in order, so an Event delivered after
   * them proves it: `marker` (default a `job-merged`, delivered to a Plan Lead; a Worker needs its own kind) is emitted last, with the id after the highest so far
   * (a test's next Event must have a higher one, as the cursor has passed it), waited for and taken out of what the pi received, so the test can assert what is left. Replaces waiting a fixed time for something that must not happen.
   */
  async drain(marker: (id: number) => unknown = (id) => ev(id, "job-merged", { data: { turn: true } })): Promise<void> {
    const id = this.lastId + 1;
    this.emit(marker(id));
    const at = await waitFor(`the marker Event ${id}`, () => {
      const i = this.pi.sent.findIndex((s) => (s.message.details as any)?.id === id);
      return i >= 0 ? { i } : undefined;
    });
    this.pi.sent.splice(at.i, 1);
    await this.quiet();
  }

  /** The other pis `load` was given: `close` shuts them down too, so no follower of theirs outlives the test. */
  private readonly others = new Set<FakePi>();

  async close(): Promise<void> {
    for (const other of this.others) await other.fire("session_shutdown").catch(() => {});
    // A reload command still running (its `reload()` shuts down and starts the adapter) would start a follower after this shutdown.
    do {
      await Promise.allSettled([...this.pi.commandsRunning]);
      await this.pi.fire("session_shutdown").catch(() => {});
    } while (this.pi.commandsRunning.size > 0);
    try {
      // `session_shutdown` SIGTERMs the followers without waiting; a call still in flight with a delay is ended, as pi's would at the end of its process.
      this.cpd.cancelPending();
      await waitFor("every fake follower to exit", () => this.cpd.followersRunning.size === 0, 10_000);
      await waitFor("every fake cpd call to end", () => this.pi.execsRunning.size === 0 && this.pi.childrenRunning.size === 0, 10_000);
    } finally {
      this.watcher.close();
      if (active === this.cpd) active = undefined;
      for (const [k, v] of Object.entries(this.saved)) {
        if (v === undefined) delete process.env[k];
        else process.env[k] = v;
      }
    }
    rmSync(this.dir, { recursive: true, force: true });
  }
}

export const planShow = (over: Record<string, unknown> = {}) => ({ ok: true, plan: { id: "p1" }, jobs: [], workflows: [], escalations: [], review: null, ...over });

export const ev = (id: number, kind: string, over: Record<string, unknown> = {}) => ({
  id, at: "2026-10-14T21:41:02.114Z", kind, context: "ctx", plan: "p1", job: null, attempt: null, session: null, text: `text of ${kind} ${id}`, data: {}, ...over,
});

export type FakeDialog = { kind: "select" | "editor"; title: string; options: string[] | undefined; signal: AbortSignal | undefined; settled: boolean; answer(value: string | undefined): void };

/** The options of `sendUserMessage`: `deliverAs` is what the question guard sends its nudge with. */
export type UserMessageOptions = { expandPromptTemplates?: boolean; deliverAs?: "steer" | "followUp" };

/** What a fake `classify` was called with. */
export type ClassifyCall = { model: { provider: string; id: string }; context: any; options: any };
/** What a fake `classify` answers: pi-ai's `ClassifyResult` shape (`stopReason`, `answers`). */
export type ClassifyBehavior = (call: ClassifyCall) => Promise<unknown>;

/** A classifier that answered: `asking_user` is a bool with `probability`. */
export const classified = (probability: number): ClassifyBehavior => () => Promise.resolve({ stopReason: "stop", answers: { asking_user: { type: "bool", probability } } });
/** A classifier call that failed (pi-ai's `error`: the endpoint is unreachable). */
export const classifyError: ClassifyBehavior = () => Promise.resolve({ stopReason: "error", errorMessage: "fetch failed", answers: {} });
/** A classifier call that ended `aborted` at once, as the 1500 ms timeout of the call's signal does (the run's own signal is not aborted). */
export const classifyTimedOut: ClassifyBehavior = () => Promise.resolve({ stopReason: "aborted", answers: {} });
/** A classifier call that throws. */
export const classifyThrows: ClassifyBehavior = () => Promise.reject(new Error("classifier blew up"));
/** A classifier call that never settles until its `signal` aborts, then ends `aborted`. */
export const classifyUntilAborted: ClassifyBehavior = ({ options }) =>
  new Promise((resolve) => {
    const done = () => resolve({ stopReason: "aborted", answers: {} });
    if (options?.signal?.aborted) done();
    else options?.signal?.addEventListener("abort", done, { once: true });
  });

export type Sent = { message: Message; options: SendOptions };

export class FakePi implements PiApi {
  /** `cpd` is the fake `cpd` its `exec` runs against: the Fixture's, unless one is given. */
  private readonly own: FakeCpd | undefined;

  constructor(own?: FakeCpd) {
    this.own = own;
  }

  private get cpd(): FakeCpd {
    const cpd = this.own ?? active;
    if (cpd === undefined) throw new Error("no fake cpd: a Fixture makes one, and a FakePi made without one runs against the Fixture alive");
    return cpd;
  }

  private handlers = new Map<string, ((event: any, ctx: Ctx) => unknown)[]>();
  private listeners = new Map<string, ((data: unknown) => void)[]>();
  readonly sent: Sent[] = [];
  /** The session's entries, as pi's `getEntries()` returns them: messages and custom entries appended so far. */
  entries: unknown[] = [];
  readonly tools: ToolDefinition<any, any, any>[] = [];
  readonly notices: string[] = [];
  /** The notices shown with level `error`. */
  readonly errorNotices: string[] = [];
  /** Every notice with the level it was shown with. */
  readonly noticeLevels: { message: string; level: string | undefined }[] = [];
  readonly emitted: { channel: string; data: any }[] = [];
  sessionName: string | undefined;
  idle = true;
  pendingMessages = false;
  editorText = "";
  /** How many times a command's `ctx.reload()` was called. */
  reloads = 0;
  /** What `ctx.reload()` does: by default nothing, like pi's mid-turn reload. */
  onReload: () => void | Promise<void> = () => {};
  readonly commands = new Map<string, (args: string, ctx: CommandCtx) => Promise<void>>();
  readonly commandErrors: unknown[] = [];
  /** Commands started by `sendUserMessage` and not finished. */
  readonly commandsRunning = new Set<Promise<void>>();
  readonly userMessages: { content: string; options: UserMessageOptions }[] = [];
  mode: Ctx["mode"] = "tui";
  sessionId = ID;

  readonly events = {
    emit: (channel: string, data: unknown) => {
      this.emitted.push({ channel, data });
      notify();
      for (const l of [...(this.listeners.get(channel) ?? [])]) l(data);
    },
    on: (channel: string, handler: (data: unknown) => void) => {
      this.listeners.set(channel, [...(this.listeners.get(channel) ?? []), handler]);
      return () => this.listeners.set(channel, (this.listeners.get(channel) ?? []).filter((h) => h !== handler));
    },
  };

  /** pi types `on` as one overload per event; the fake keeps every handler by name and fires it with a `Ctx`. */
  readonly on = ((event: string, handler: (event: any, ctx: Ctx) => unknown): (() => void) => {
    this.handlers.set(event, [...(this.handlers.get(event) ?? []), handler]);
    return () => {};
  }) as PiApi["on"];

  /** The `exec` calls (argv, without the command) started and not yet finished: a call is in flight from the moment the adapter makes it, however long the process takes to start. */
  readonly execsRunning = new Set<string[]>();

  /** Every process the adapter started (a follower) that node has not yet seen close. */
  readonly childrenRunning = new Set<ChildProcess>();

  track<T extends ChildProcess>(child: T): T {
    this.childrenRunning.add(child);
    child.once("close", () => {
      this.childrenRunning.delete(child);
      notify();
    });
    return child;
  }

  /** Like pi's `execCommand`, answered in process by the fake `cpd`: `timeout` and `signal` end the call and the result says `killed` (its `code` is then not an exit code). */
  async exec(command: string, args: string[], options: ExecOptions = {}): Promise<ExecResult> {
    if (command !== "cpd") throw new Error(`the fake pi runs only cpd, not ${command}`);
    this.execsRunning.add(args);
    this.execOptions.push({ command, args, options });
    notify();
    try {
      return await this.cpd.call(args, options);
    } finally {
      this.execsRunning.delete(args);
      notify();
    }
  }

  /** What `exec` was called with, in order. */
  readonly execOptions: { command: string; args: string[]; options: ExecOptions }[] = [];

  readonly sendMessage: PiApi["sendMessage"] = (message, options) => {
    if (!options || typeof message.content !== "string") throw new Error("the adapter always passes send options and a string content");
    this.sent.push({ message: { ...message, content: message.content }, options });
    this.entries.push({ type: "custom_message", customType: message.customType, content: message.content, display: message.display, details: message.details });
    notify();
  };

  appendEntry(customType: string, data: unknown): void {
    this.entries.push({ type: "custom", customType, data });
    notify();
  }

  setSessionName(name: string): void {
    this.sessionName = name;
    notify();
  }

  getSessionName(): string | undefined {
    return this.sessionName;
  }

  /** The renderers registered, by custom type, with pi's own renderer type. */
  readonly renderers = new Map<string, Parameters<PiApi["registerMessageRenderer"]>[1]>();

  readonly registerMessageRenderer: PiApi["registerMessageRenderer"] = (customType, renderer) => {
    this.renderers.set(customType, renderer as Parameters<PiApi["registerMessageRenderer"]>[1]);
  };

  readonly registerTool: PiApi["registerTool"] = (tool) => {
    this.tools.push(tool);
  };

  /** Runs a registered tool like pi does. The adapter's tool reads nothing of the tool context, so the fake passes none of pi's. */
  async runTool(tool: ToolDefinition<any, any, any>, params: Record<string, unknown> = {}): Promise<{ text: string; details: unknown }> {
    const out = await tool.execute("call-1", params, undefined, undefined, undefined as never);
    return { text: out.content.map((c) => (c.type === "text" ? c.text : "")).join(""), details: out.details };
  }

  registerCommand(name: string, options: { handler: (args: string, ctx: CommandCtx) => Promise<void> }): void {
    this.commands.set(name, options.handler);
  }

  /** Like pi: with `expandPromptTemplates` a registered extension command runs at once (not awaited by the sender). */
  sendUserMessage(content: string, options: UserMessageOptions): void {
    this.userMessages.push({ content, options });
    notify();
    const handler = options.expandPromptTemplates === true && content.startsWith("/") ? this.commands.get(content.slice(1).split(" ")[0]!) : undefined;
    if (!handler) return;
    const running: Promise<void> = handler("", this.commandCtx())
      .catch((e) => void this.commandErrors.push(e))
      .finally(() => {
        this.commandsRunning.delete(running);
        notify();
      });
    this.commandsRunning.add(running);
    notify();
  }

  /** What `ctx.isProjectTrusted()` answers. */
  trusted = false;

  /** The dialogs the adapter opened with `ctx.ui.select` and `ctx.ui.editor`, in order. Like pi's, a select with a `signal` that aborts answers `undefined`; an editor has no signal. */
  readonly dialogs: FakeDialog[] = [];

  /** The oldest dialog of `kind` still waiting for an answer. */
  openDialog(kind: "select" | "editor"): FakeDialog | undefined {
    return this.dialogs.find((d) => d.kind === kind && !d.settled);
  }

  private dialog(kind: "select" | "editor", title: string, options: string[] | undefined, signal: AbortSignal | undefined): Promise<string | undefined> {
    return new Promise((resolve) => {
      const d: FakeDialog = {
        kind, title, options, signal, settled: false,
        answer: (value) => {
          if (d.settled) return;
          d.settled = true;
          resolve(value);
          notify();
        },
      };
      this.dialogs.push(d);
      notify();
      if (signal?.aborted) d.answer(undefined);
      else signal?.addEventListener("abort", () => d.answer(undefined), { once: true });
    });
  }

  /** The models `pi.setModel` was called with, in order (commands.md §15.3 `session-retiered`), and the levels `pi.setThinkingLevel` was called with. */
  readonly modelsSet: { provider: string; id: string }[] = [];
  readonly thinkingSet: string[] = [];
  /** The `registerMcpServer` calls (commands.md §15.3 `mcp-attached`), in order; `mcpThrows` is thrown by each when set. */
  readonly registeredMcp: { name: string; config: unknown }[] = [];
  mcpThrows?: Error;
  /** Names `registerMcpServer` refuses (throws for), the others are accepted. */
  readonly mcpThrowsFor = new Set<string>();
  registerMcpServer(name: string, config: unknown): void {
    if (this.mcpThrowsFor.has(name)) {
      notify();
      throw new Error(`pi refuses: ${name}`);
    }
    this.registeredMcp.push({ name, config });
    notify();
    if (this.mcpThrows) throw this.mcpThrows;
    if (this.mcpThrowsFor.has(name)) throw new Error(`pi refuses: ${name}`);
  }
  /** What `setModel` answers: false is pi's "no authentication for the model's provider". */
  modelAccepted = true;
  /** The models `ctx.modelRegistry.find(provider, id)` knows, as `provider/id`. */
  readonly knownModels = new Set<string>(["p/opus", "p/sonnet", "p/haiku"]);
  /** The classifier models `ctx.modelRegistry.findOfType("classifier", provider, id)` knows, as `provider/id`. */
  readonly classifierModels = new Set<string>(["llama.cpp/kev-4b"]);
  /** Every `ctx.modelRegistry.classify` call, in order. */
  readonly classifyCalls: ClassifyCall[] = [];
  /** What `classify` does: by default it answers with probability 0.99. A test sets it, and may set it again between calls. */
  classifyBehavior: ClassifyBehavior = classified(0.99);

  async setModel(model: { provider: string; id: string }): Promise<boolean> {
    this.modelsSet.push(model);
    notify();
    return this.modelAccepted;
  }

  setThinkingLevel(level: string): void {
    this.thinkingSet.push(level);
    notify();
  }

  ctx(): Ctx {
    const ui = {
      notify: (m: string, level?: string) => {
        this.notices.push(m);
        this.noticeLevels.push({ message: m, level });
        notify();
        if (level === "error") this.errorNotices.push(m);
      },
      setStatus: () => {},
      setHeader: () => {},
      setWidget: () => {},
      getEditorText: () => this.editorText,
      select: (title: string, options: string[], opts?: { signal?: AbortSignal }) => this.dialog("select", title, options, opts?.signal),
      editor: (title: string) => this.dialog("editor", title, undefined, undefined),
    };
    // `modelRegistry` is what the adapter reads to find a model (commands.md §15.3 `session-retiered`): pi's `ModelRegistry.find`.
    const ctx: Ctx & { modelRegistry: { find(provider: string, id: string): { provider: string; id: string } | undefined; findOfType(kind: string, provider: string, id: string): { provider: string; id: string } | undefined; classify(model: { provider: string; id: string }, context: unknown, options?: unknown): Promise<unknown> } } = {
      mode: this.mode,
      cwd: join(tmpdir(), "pi-test-no-such-project"),
      isProjectTrusted: () => this.trusted,
      ui,
      sessionManager: { getSessionId: () => this.sessionId, getEntries: () => this.entries as ReturnType<Ctx["sessionManager"]["getEntries"]>, getBranch: () => this.entries as ReturnType<Ctx["sessionManager"]["getBranch"]> },
      isIdle: () => this.idle,
      hasPendingMessages: () => this.pendingMessages,
      modelRegistry: {
        find: (provider, id) => (this.knownModels.has(`${provider}/${id}`) ? { provider, id } : undefined),
        findOfType: (kind, provider, id) => (kind === "classifier" && this.classifierModels.has(`${provider}/${id}`) ? { provider, id } : undefined),
        classify: (model, context, options) => {
          const call = { model, context, options };
          this.classifyCalls.push(call);
          notify();
          return this.classifyBehavior(call);
        },
      },
    };
    return ctx;
  }

  /** A command context that, like pi's, throws on any use once `reload()` has resolved. */
  commandCtx(): CommandCtx {
    let stale = false;
    const ctx: CommandCtx = {
      ...this.ctx(),
      reload: async () => {
        this.reloads++;
        notify();
        await this.onReload();
        stale = true;
      },
    };
    return new Proxy(ctx, {
      get: (target, key) => {
        if (stale) throw new Error(`stale ctx: .${String(key)} used after reload()`);
        return Reflect.get(target, key);
      },
    });
  }

  async fire(event: string, payload: Record<string, unknown> = {}): Promise<void> {
    for (const h of this.handlers.get(event) ?? []) await h({ type: event, ...payload }, this.ctx());
  }

  /** Like `fire`, returning what each handler returned (`before_agent_start` returns the message to add). The adapter's `markReply`, `policyHandler` and `loopGuardHandler` observers add no message and are left out, so the list stays what the handlers that add a message returned. */
  async fireWith(event: string, payload: Record<string, unknown> = {}): Promise<unknown[]> {
    const out: unknown[] = [];
    for (const h of this.handlers.get(event) ?? []) {
      const result = await h({ type: event, ...payload }, this.ctx());
      if (h.name !== "markReply" && h.name !== "policyHandler" && h.name !== "timeZoneHandler" && h.name !== "loopGuardHandler") out.push(result);
    }
    return out;
  }

  /** A run is active and a tool call is in progress: what a busy Lead looks like to the adapter. */
  async busy(): Promise<void> {
    this.idle = false;
    await this.fire("agent_start");
    await this.fire("tool_execution_start", { toolCallId: "t1", toolName: "bash", args: {} });
  }

  /** `cpd` messages delivered (not the adapter's own errors). */
  delivered(): Sent[] {
    return this.sent.filter((s) => s.message.customType === "cpd" && (s.message.details as any)?.delivered_to !== undefined);
  }

  errors(): Sent[] {
    return this.sent.filter((s) => s.message.customType === "cpd" && (s.message.details as any)?.kind === "error");
  }
}

/**
 * Resolves with the first truthy `fn()`, looked at once now and again after every {@link notify} (once the work that caused it has run to its next macrotask).
 * `ms` only fails a hang: a rejection naming `what`. Nothing polls.
 */
export function waitFor<T>(what: string, fn: () => T | undefined | null | false, ms = 15000): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    let queued = false;
    let settled = false;
    const stop = () => {
      settled = true;
      watchers.delete(wake);
      realTimers.clearTimeout(hang);
    };
    const look = () => {
      queued = false;
      if (settled) return;
      try {
        const v = fn();
        if (!v) return;
        stop();
        resolve(v);
      } catch (e) {
        stop();
        reject(e);
      }
    };
    const wake = () => {
      if (queued) return;
      queued = true;
      setImmediate(look);
    };
    const onHang = () => {
      stop();
      reject(new Error(`timed out after ${ms} ms waiting for ${what}`));
    };
    watchers.add(wake);
    // The one timer of a wait, real whatever the fake's `timers` are, and only to fail a hang; it also keeps node alive while the adapter's own (unref'd) timers are what will end the wait.
    const hang = realTimers.setTimeout(onHang, ms);
    look();
  });
}

/** `session state` argvs, without the session id, as `idle`, `blocked:<label>` or `waiting:<label>`. */
export const stateCalls = (f: Fixture): string[] =>
  f.argvs().filter((a) => a[0] === "session" && a[1] === "state").map((a) => (a[2] === "blocked" || a[2] === "waiting" ? `${a[2]}:${a[a.indexOf("--label") + 1]}` : a[2]!));

/** Resolves when pi asked for `count` transports (it connects every server it knows in the background after `session_start`; `transports` is the harness's record). */
export function transportsStarted(transports: readonly unknown[], count = 1): Promise<void> {
  return waitFor(`${count} transport(s) pi connects`, () => transports.length >= count, 5000).then(() => {});
}
