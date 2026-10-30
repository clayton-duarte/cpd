import { spawn as nodeSpawn, type ChildProcess } from "node:child_process";
import { CpdError } from "./cpd.ts";

export type Timing = {
  /** First restart delay of the follower; doubles up to `restartMaxMs`. */
  restartInitialMs: number;
  restartMaxMs: number;
  /** How long `session_shutdown` waits for `session state stopped`. */
  shutdownWaitMs: number;
  /** The deadline of one `cpd` call (commands.md §15.1, "Calls"). */
  callTimeoutMs: number;
};

export const DEFAULT_TIMING: Timing = { restartInitialMs: 1_000, restartMaxMs: 30_000, shutdownWaitMs: 2_000, callTimeoutMs: 30_000 };

/**
 * Error codes that are a refusal of the call itself (commands.md §1.2, §11.2): the same call fails the same way every time,
 * so nothing is retried. Every other code (a busy or too-new store, a spawn failure, an exit without a line) may pass.
 */
export const PERMANENT_CODES: ReadonlySet<string> = new Set([
  "usage",
  "session_required",
  "invalid_session_id",
  "session_ambiguous",
  "session_not_found",
  "plan_not_found",
  "context_not_found",
  "cursor_ahead",
]);

export type Spawn = typeof nodeSpawn;

/** How the restart is scheduled; tests pass their own to see the delays instead of waiting for them. */
export type Schedule = (run: () => void, ms: number) => ReturnType<typeof setTimeout>;

export type FollowerHooks = {
  /** Awaited before every process is started, the first included (the protocol check). It must not throw. */
  before(): Promise<void>;
  /** A process was started. */
  onRunStart(): void;
  /** Each complete stdout line. Returns true when the restart delay resets: a valid Event, or a failure line the restart cures: only that resets the restart delay. */
  onLine(line: string): boolean;
  /** The process failed without a failure line: a spawn error or a non-zero exit. */
  onFailure(error: CpdError): void;
  /** The cursor for the next `--since`. */
  cursor(): number;
};

/**
 * commands.md §15.2: `cpd events follow --session <id> --since <cursor>` as a child process (argv, stdin `/dev/null`),
 * read line by line. When it ends it is restarted with the in-memory cursor after `restartInitialMs`,
 * backing off to `restartMaxMs`; reading a valid Event resets the delay.
 */
export class Follower {
  private child: ChildProcess | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private delay: number;
  private stopped = false;
  /** Bumped by `start` and `stop`, so a launch waiting on `before` knows it was replaced. */
  private generation = 0;
  private readonly sessionId: string;
  private readonly hooks: FollowerHooks;
  private readonly timing: Timing;
  private readonly spawn: Spawn;
  private readonly schedule: Schedule;

  constructor(sessionId: string, hooks: FollowerHooks, timing: Timing, spawn: Spawn = nodeSpawn, schedule: Schedule = setTimeout) {
    this.sessionId = sessionId;
    this.hooks = hooks;
    this.timing = timing;
    this.spawn = spawn;
    this.schedule = schedule;
    this.delay = timing.restartInitialMs;
  }

  start(): void {
    this.stopped = false;
    void this.launch();
  }

  stop(): void {
    this.stopped = true;
    this.generation++;
    if (this.timer) clearTimeout(this.timer);
    this.timer = undefined;
    this.child?.kill("SIGTERM");
    this.child = undefined;
  }

  private async launch(): Promise<void> {
    const generation = ++this.generation;
    await this.hooks.before();
    if (this.generation !== generation) return; // stopped or restarted meanwhile
    this.hooks.onRunStart();
    const args = ["events", "follow", "--session", this.sessionId, "--since", String(this.hooks.cursor())];
    let child: ChildProcess;
    try {
      child = this.spawn("cpd", args, { stdio: ["ignore", "pipe", "pipe"] });
    } catch (e) {
      this.hooks.onFailure(new CpdError("spawn_failed", `cannot run cpd ${args.join(" ")}: ${e instanceof Error ? e.message : String(e)}`));
      this.restartLater();
      return;
    }
    this.child = child;
    let buffer = "";
    let stderr = "";
    let ended = false;
    const lines = (flush: boolean) => {
      let nl: number;
      while ((nl = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, nl);
        buffer = buffer.slice(nl + 1);
        if (line.trim() !== "") this.read(line);
      }
      if (flush && buffer.trim() !== "") this.read(buffer);
      if (flush) buffer = "";
    };
    child.stdout?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => {
      if (this.child !== child) return;
      buffer += chunk;
      lines(false);
    });
    child.stderr?.setEncoding("utf8");
    child.stderr?.on("data", (chunk: string) => {
      stderr = (stderr + chunk).slice(-500);
    });
    const end = (failure: CpdError | undefined) => {
      if (ended) return;
      ended = true;
      if (this.child !== child) return; // stopped, or replaced
      lines(true);
      this.child = undefined;
      if (failure) this.hooks.onFailure(failure);
      this.restartLater();
    };
    child.on("error", (e) => end(new CpdError("spawn_failed", `cannot run cpd ${args.join(" ")}: ${e.message}`)));
    child.on("close", (code, signal) => {
      end(code === 0 ? undefined : new CpdError("follower_exit", `cpd events follow ended with ${code === null ? `signal ${signal}` : `exit ${code}`}${stderr.trim() ? `: ${stderr.trim()}` : ""}`, { exit: code }));
    });
  }

  private read(line: string): void {
    if (this.hooks.onLine(line)) this.delay = this.timing.restartInitialMs;
  }

  private restartLater(): void {
    if (this.stopped) return;
    const wait = this.delay;
    this.delay = Math.min(this.delay * 2, this.timing.restartMaxMs);
    this.timer = this.schedule(() => {
      this.timer = undefined;
      if (!this.stopped) void this.launch();
    }, wait);
  }
}
