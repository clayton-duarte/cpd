import { type ChildProcess, execFile, spawn } from "node:child_process";
import { appendFileSync, chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { adapter, type Options } from "../lib/adapter.ts";
import { PROTOCOL } from "../lib/event.ts";
import type { CommandCtx, Ctx, ExecOptions, ExecResult, Message, MessageRenderer, PiApi, SendOptions, ToolDefinition } from "../lib/pi.ts";

/** A fake `cpd` on PATH and a fake pi, so the adapter is tested through its real boundaries (child processes, pi's API). */

const FAKE_CPD = `#!/usr/bin/env node
const fs = require("fs");
const path = require("path");
const dir = process.env.FAKE_CPD_DIR;
const argv = process.argv.slice(2);
const log = (o) => fs.appendFileSync(path.join(dir, "calls.jsonl"), JSON.stringify({ ...o, t: Date.now() }) + "\\n");
const read = (f) => { try { return fs.readFileSync(path.join(dir, f), "utf8"); } catch { return undefined; } };
const flag = (n) => { const i = argv.indexOf(n); return i >= 0 ? argv[i + 1] : undefined; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function follow() {
  const since = Number(flag("--since"));
  // the handler first: once the pid is visible a SIGTERM may come, and before the handler it would kill without a trace
  process.on("SIGTERM", () => { fs.writeFileSync(path.join(dir, "killed-" + process.pid), "1"); process.exit(143); });
  fs.appendFileSync(path.join(dir, "follower.pids"), process.pid + "\\n");
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
  const r = JSON.parse(read("resp/" + key + "@" + flag("--kind") + ".json") || read("resp/" + key + "-" + argv[2] + ".json") || read("resp/" + key + ".json") || '{"stdout":{"ok":true}}');
  let feedback;
  if (flag("--feedback-file")) feedback = fs.readFileSync(flag("--feedback-file"), "utf8");
  if (r.delay) await sleep(r.delay);
  log({ phase: "end", argv, feedback });
  process.stdout.write(typeof r.stdout === "string" ? r.stdout : JSON.stringify(r.stdout));
  process.exit(r.exit || 0);
})();
`;

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
fs.appendFileSync(path.join(process.env.FAKE_CPD_DIR, "validate-calls.jsonl"), JSON.stringify({ argv, cwd: process.cwd(), agentDir: process.env.PI_CODING_AGENT_DIR, offline: process.env.PI_OFFLINE, agentDirExists: fs.existsSync(process.env.PI_CODING_AGENT_DIR) }) + "\\n");
import(entry).then((m) => {
  if (typeof m.default !== "function") throw new Error("Extension does not export a valid factory function: " + entry);
  process.exit(0);
}).catch((e) => {
  console.error('Error: Failed to load extension "' + entry + '": ' + (e instanceof Error ? e.message : String(e)) + "\\nHint: Start without extensions using \\"pi -ne\\".");
  process.exit(1);
});
`;

export const ID = "596d007f-6775-4134-b77a-b356cc523842";
export const OTHER = "11111111-2222-4333-8444-555555555555";

export type Call = { phase: "start" | "end"; argv: string[]; since?: string; feedback?: string; t: number };

export class Fixture {
  readonly dir = mkdtempSync(join(tmpdir(), "pi-test-"));
  readonly cpdHome = join(this.dir, "cpd-home");
  /** Stands for the adapter's installed directory (`options.reload.dir`). */
  readonly adapterDir = join(this.dir, "adapter");
  readonly agentDir = join(this.dir, "agent");
  readonly pi: FakePi;
  private readonly saved: Record<string, string | undefined> = {};

  constructor() {
    mkdirSync(join(this.dir, "bin"));
    mkdirSync(join(this.dir, "resp"));
    mkdirSync(this.cpdHome);
    mkdirSync(this.adapterDir);
    writeFileSync(join(this.dir, "bin", "pi"), FAKE_PI);
    chmodSync(join(this.dir, "bin", "pi"), 0o755);
    writeFileSync(join(this.dir, "bin", "cpd"), FAKE_CPD);
    chmodSync(join(this.dir, "bin", "cpd"), 0o755);
    for (const k of ["PATH", "FAKE_CPD_DIR", "CPD_SESSION", "CPD_PLAN", "CPD_HOME", "PI_CODING_AGENT_DIR"]) this.saved[k] = process.env[k];
    process.env.PATH = `${join(this.dir, "bin")}:${process.env.PATH}`;
    process.env.FAKE_CPD_DIR = this.dir;
    process.env.CPD_SESSION = ID;
    process.env.CPD_HOME = this.cpdHome;
    process.env.PI_CODING_AGENT_DIR = this.agentDir; // pi's agent directory: where the adapter reads `compact-tools.json` (commands.md §15.9)
    delete process.env.CPD_PLAN;
    this.pi = new FakePi();
    this.respond("session-show", sessionShow());
    this.respond("context-pending", { ok: true, context: "ctx", plans: [] }); // a Manager reads it at every turn start (§15.7)
    this.respond("--version", { ok: true, version: "0.0.0", schema: 6, store_schema: 6, protocol: PROTOCOL });
  }

  /** A new install of the fake `cpd`: the same program in a file of another size and mtime. */
  reinstall(): void {
    const file = join(this.dir, "bin", "cpd");
    appendFileSync(file, `// install ${++this.installs}\n`);
    const later = new Date(Date.now() + 1000 * this.installs);
    utimesSync(file, later, later);
  }
  private installs = 0;

  /** What `cpd <a> <b>` prints (an object, or a raw string) and exits with. Key `<a>-<b>`; `<a>-<b>-<c>` answers `cpd <a> <b> <c>` first; `<a>-<b>@<kind>` answers a call whose first `--kind` is `<kind>`, before both. */
  respond(key: string, stdout: unknown, extra: { exit?: number; delay?: number } = {}): void {
    this.write(join("resp", `${key}.json`), JSON.stringify({ stdout, ...extra }));
  }

  /** Appends Event lines (objects or raw strings) to what the fake follower prints. */
  emit(...lines: unknown[]): void {
    for (const l of lines) appendFileSync(join(this.dir, "events.jsonl"), (typeof l === "string" ? l : JSON.stringify(l)) + "\n");
  }

  file(name: string, content: string): void {
    this.write(name, content);
  }

  /** Atomic (write, then rename): the fake `cpd` polls these files and must never read one half written. */
  private write(name: string, content: string): void {
    const tmp = join(this.dir, `.tmp-${++this.writes}`);
    writeFileSync(tmp, content);
    renameSync(tmp, join(this.dir, name));
  }
  private writes = 0;

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
    return this.calls().filter((c) => c.phase === "start" && c.argv[0] !== "events" && c.argv[0] !== "--version").map((c) => c.argv);
  }

  /** How often the adapter ran `cpd --version`. */
  versions(): number {
    return this.calls().filter((c) => c.phase === "start" && c.argv[0] === "--version").length;
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
    const start = options.spawn ?? spawn;
    adapter(this.pi, {
      ...options,
      spawn: ((...args: Parameters<typeof spawn>) => this.pi.track(start(...args))) as typeof spawn,
      timing: { restartInitialMs: 30, restartMaxMs: 120, requestTimeoutMs: 300, pollMs: 30, shutdownWaitMs: 200, callTimeoutMs: 10_000, ...options.timing },
      reload: { dir: this.adapterDir, intervalMs: 20, ...options.reload },
    });
  }

  async close(): Promise<void> {
    // A reload command still running (its `reload()` shuts down and starts the adapter) would start a follower after this shutdown.
    do {
      await Promise.allSettled([...this.pi.commandsRunning]);
      await this.pi.fire("session_shutdown").catch(() => {});
    } while (this.pi.commandsRunning.size > 0);
    try {
      // `session_shutdown` SIGTERMs the followers without waiting, and the fake's handler still writes `killed-<pid>` into the dir: remove it only once every follower is gone.
      await waitFor("every fake follower to exit", () => this.pids().every((pid) => !alive(pid)), 10_000);
      // A `cpd` process (a one-shot call such as a poll's `--version`, or a follower not yet in `follower.pids`) logs into the dir until it is gone.
      await waitFor("every fake cpd process to close", () => this.pi.childrenRunning.size === 0, 10_000);
    } finally {
      for (const [k, v] of Object.entries(this.saved)) {
        if (v === undefined) delete process.env[k];
        else process.env[k] = v;
      }
    }
    rmSync(this.dir, { recursive: true, force: true });
  }
}

/** Whether a process exists (signal 0 only checks). A child not yet reaped by node still counts: it is gone once node has seen it exit. */
function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ESRCH") return false;
    throw e;
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
  /** The notices shown with level `error`. */
  readonly errorNotices: string[] = [];
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
  readonly userMessages: { content: string; options: { expandPromptTemplates: boolean } }[] = [];
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

  /** The `exec` calls (argv, without the command) started and not yet finished: a call is in flight from the moment the adapter makes it, however long the process takes to start. */
  readonly execsRunning = new Set<string[]>();

  /** Every process the adapter started (a call or a follower) that node has not yet seen close. `execFile` answers a call aborted or timed out at once, while its killed process may still be starting up, so `execsRunning` is not enough to know nothing more is written into the dir. */
  readonly childrenRunning = new Set<ChildProcess>();

  track<T extends ChildProcess>(child: T): T {
    this.childrenRunning.add(child);
    child.once("close", () => this.childrenRunning.delete(child));
    return child;
  }

  /** Like pi's `execCommand`: `timeout` and `signal` SIGTERM the process, and the result says `killed` (its `code` is then not an exit code). */
  exec(command: string, args: string[], options: ExecOptions = {}): Promise<ExecResult> {
    this.execsRunning.add(args);
    this.execOptions.push({ command, args, options });
    return new Promise((resolve) => {
      this.track(execFile(command, args, { ...options, encoding: "utf8" }, (err, stdout, stderr) => {
        this.execsRunning.delete(args);
        const killed = err !== null && (err.killed === true || err.name === "AbortError");
        resolve({ stdout, stderr, code: killed ? 0 : err ? (typeof err.code === "number" ? err.code : 1) : 0, killed });
      }));
    });
  }

  /** What `exec` was called with, in order. */
  readonly execOptions: { command: string; args: string[]; options: ExecOptions }[] = [];

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

  readonly renderers = new Map<string, MessageRenderer>();

  registerMessageRenderer(customType: string, renderer: MessageRenderer): void {
    this.renderers.set(customType, renderer);
  }

  registerTool(tool: ToolDefinition): void {
    this.tools.push(tool);
  }

  registerCommand(name: string, options: { handler: (args: string, ctx: CommandCtx) => Promise<void> }): void {
    this.commands.set(name, options.handler);
  }

  /** Like pi: with `expandPromptTemplates` a registered extension command runs at once (not awaited by the sender). */
  sendUserMessage(content: string, options: { expandPromptTemplates: boolean }): void {
    this.userMessages.push({ content, options });
    const handler = options.expandPromptTemplates && content.startsWith("/") ? this.commands.get(content.slice(1).split(" ")[0]!) : undefined;
    if (!handler) return;
    const running: Promise<void> = handler("", this.commandCtx())
      .catch((e) => void this.commandErrors.push(e))
      .finally(() => void this.commandsRunning.delete(running));
    this.commandsRunning.add(running);
  }

  /** What `ctx.isProjectTrusted()` answers. */
  trusted = false;

  ctx(): Ctx {
    return {
      mode: this.mode,
      cwd: join(tmpdir(), "pi-test-no-such-project"),
      isProjectTrusted: () => this.trusted,
      ui: { notify: (m, level) => {
          this.notices.push(m);
          if (level === "error") this.errorNotices.push(m);
        }, getEditorText: () => this.editorText },
      sessionManager: { getSessionId: () => this.sessionId, getEntries: () => this.entries },
      isIdle: () => this.idle,
      hasPendingMessages: () => this.pendingMessages,
    };
  }

  /** A command context that, like pi's, throws on any use once `reload()` has resolved. */
  commandCtx(): CommandCtx {
    let stale = false;
    const ctx: CommandCtx = {
      ...this.ctx(),
      reload: async () => {
        this.reloads++;
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

  /** Like `fire`, returning what each handler returned (`before_agent_start` returns the message to add). The adapter's `markReply` observer (reply.ts) adds nothing and is left out, so the list stays what the handlers that add a message returned. */
  async fireWith(event: string, payload: Record<string, unknown> = {}): Promise<unknown[]> {
    const out: unknown[] = [];
    for (const h of this.handlers.get(event) ?? []) {
      const result = await h({ type: event, ...payload }, this.ctx());
      if (h.name !== "markReply") out.push(result);
    }
    return out;
  }

  /** `cpd` messages delivered (not the adapter's own errors). */
  delivered(): Sent[] {
    return this.sent.filter((s) => s.message.customType === "cpd" && (s.message.details as any)?.delivered_to !== undefined);
  }

  errors(): Sent[] {
    return this.sent.filter((s) => s.message.customType === "cpd" && (s.message.details as any)?.kind === "error");
  }
}

export async function waitFor<T>(what: string, fn: () => T | undefined | null | false, ms = 15000): Promise<T> {
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
