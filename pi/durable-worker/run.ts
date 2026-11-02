import type { Extension } from "@earendil-works/pi-durable";
import type { FollowerHooks } from "../lib/follower.ts";
import { CpdError } from "../lib/cpd.ts";
import { parseLine } from "../lib/event.ts";
import { policySection } from "../lib/policy.ts";
import { parseSession } from "../lib/run.ts";
import { Reporter, stateArgs, type Wanted } from "../lib/state.ts";
import { zoneLine } from "../lib/time.ts";
import { reportPath, storePath, type Config } from "./config.ts";
import { inputOf, type Input } from "./deliver.ts";
import { codemodeNamespaces } from "../durable-host/codemode.ts";
import { cpdExtension } from "./extension.ts";
import { mcpHandler, type McpPort } from "./mcp.ts";
import { endReport, type EndReport, type Entry } from "./report.ts";

export type Submission = { wait(): Promise<"done" | "unanswered"> };
export type Conversation = { submit(input: Input): Promise<Submission>; install(extension: Extension): void; entries(): Promise<readonly Entry[]>; closed: Promise<unknown> };

type Model = { provider: string; modelId: string; thinkingLevel?: string };

export type Deps = {
  cpd(args: readonly string[]): Promise<Record<string, unknown>>;
  resolveModel(pattern: string): Model;
  contextFiles(cwd: string): readonly { path: string; content: string }[];
  open(o: { file: string; cwd: string; model: Model; extension: Extension; codemode: Extension }): Promise<Conversation>;
  follow(hooks: FollowerHooks): { start(): void; stop(): void };
  writeReport(file: string, report: EndReport): Promise<void>;
  onTerminate(handler: () => void): void;
  codemode(namespaces: readonly { name: string; prefix: string }[]): Extension;
  mcp: McpPort;
  homeDir(): string;
  now(): Date;
  log(line: string): void;
};

const STATES = ["working", "waiting", "blocked", "idle", "stopped"];

const contextText = (files: readonly { path: string; content: string }[]): string | undefined =>
  files.length === 0 ? undefined : files.map((f) => `## ${f.path}\n${f.content}`).join("\n\n");

/** Runs one Attempt (contract §6.12). Resolves only on terminate (143), a closed harness (1) or a failure (1); a `usage` error of the model is thrown before any effect. */
export async function runWorker(config: Config, deps: Deps): Promise<number> {
  const model = deps.resolveModel(config.model);
  const failed = (what: string, e: unknown) => deps.log(`${what}: ${e instanceof Error ? e.message : String(e)}`);

  let end!: (code: number) => void;
  const ended = new Promise<number>((resolve) => {
    end = resolve;
  });
  let over = false;
  deps.onTerminate(() => end(143));

  const reporter = new Reporter(
    async (w: Wanted) => {
      await deps.cpd(stateArgs(w, config.session));
    },
    (e) => failed("session state", e),
  );
  let follower: { start(): void; stop(): void } | undefined;
  let opened: Conversation | undefined;
  let mcp: ReturnType<typeof mcpHandler> | undefined;

  const main = async (): Promise<void> => {
    const shownSession = await deps.cpd(["session", "show", config.session]);
    const session = parseSession(shownSession);
    const shownState = (shownSession.session as { state?: unknown }).state;
    if (typeof shownState !== "string" || !STATES.includes(shownState)) throw new CpdError("bad_output", "cpd session show: the session state is not one of working, waiting, blocked, idle or stopped");
    const policy = policySection(await deps.cpd(["policy", "show"]));
    const extension = cpdExtension(
      { contextFiles: contextText(deps.contextFiles(config.cwd)), policy, zone: zoneLine(deps.now()) },
      { cwd: config.cwd, home: deps.homeDir(), cpdHome: config.home, kind: config.jobKind, plan: config.plan },
    );
    const conversation = await deps.open({ file: storePath(config.home, config.plan, config.attempt), cwd: config.cwd, model, extension, codemode: deps.codemode([]) });
    opened = conversation;
    conversation.closed.then(
      (reason) => {
        failed("the harness closed", reason);
        end(1);
      },
      (e) => {
        failed("the harness closed", e);
        end(1);
      },
    );

    let pending = 0;
    const submit = async (input: Input): Promise<void> => {
      if (over) return;
      pending++;
      reporter.want({ state: "working" });
      const submission = await conversation.submit(input);
      void submission
        .wait()
        .then(async () => {
          pending--;
          await deps.writeReport(reportPath(config.home, config.plan, config.attempt), endReport(await conversation.entries()));
          if (pending === 0) reporter.want({ state: "idle" });
        })
        .catch((e) => {
          failed("a submission", e);
          end(1);
        });
    };

    // §3.6 allows `stopped -> idle`, never `stopped -> working`; after a crash restart the Session is not `stopped`.
    if (shownState === "stopped") reporter.want({ state: "idle" });
    await submit({ content: `Read ${config.spec} and follow it exactly. Your attempt id is ${config.attempt}.`, requestId: config.attempt, whenBusy: "followUp" });

    const handler = mcpHandler({ session: config.session, port: deps.mcp, install: (x) => conversation.install(x), cpd: deps.cpd, submit, log: deps.log, attached: (servers) => conversation.install(deps.codemode(codemodeNamespaces(servers))), over: () => over });
    mcp = handler;
    await handler.replay();

    let cursor = session.registeredEvent;
    follower = deps.follow({
      before: async () => {},
      onRunStart: () => {},
      onLine: (text) => {
        const line = parseLine(text);
        if (line.type !== "event") {
          failed("follower line", line.type === "failure" ? line.error : new CpdError("bad_output", line.type));
          return false;
        }
        cursor = line.event.id;
        if (line.event.kind === "mcp-attached") {
          handler.handle(line.event);
          return true;
        }
        const input = inputOf(line.event, config.session);
        if (input) submit(input).catch((e) => {
          failed("an event", e);
          end(1);
        });
        return true;
      },
      onFailure: (e) => failed("follower", e),
      cursor: () => cursor,
    });
    follower.start();
  };

  main().catch((e) => {
    failed("the run", e);
    end(1);
  });

  const code = await ended;
  over = true;
  follower?.stop();
  if (code === 143 && opened) {
    try {
      await deps.writeReport(reportPath(config.home, config.plan, config.attempt), endReport(await opened.entries()));
    } catch (e) {
      failed("the end report at terminate", e);
    }
  }
  await mcp?.closeAll();
  await reporter.close({ state: "stopped" });
  return code;
}
