import type { Extension } from "@earendil-works/pi-durable";
import type { FollowerHooks } from "../lib/follower.ts";
import { parseLine } from "../lib/event.ts";
import { cursorFrom } from "./cursor.ts";
import { inputOf, type Input } from "./deliver.ts";
import { CpdError } from "../lib/cpd.ts";
import { policySection } from "../lib/policy.ts";
import { parseSession } from "../lib/run.ts";
import { Reporter, stateArgs, type Wanted } from "../lib/state.ts";
import { zoneLine } from "../lib/time.ts";
import { storePath, type Config } from "./config.ts";
import { hostExtension } from "./extension.ts";

export type Submission = { wait(): Promise<"done" | "unanswered"> };
export type Conversation = { submit(input: Input): Promise<Submission>; install(extension: Extension): void; entries(): Promise<readonly unknown[]>; closed: Promise<unknown> };

type Model = { provider: string; modelId: string; thinkingLevel?: string };

export type Deps = {
  cpd(args: readonly string[]): Promise<Record<string, unknown>>;
  resolveModel(pattern: string): Model;
  contextFiles(cwd: string): readonly { path: string; content: string }[];
  open(o: { file: string; cwd: string; model: Model; extension: Extension }): Promise<Conversation>;
  follow?(hooks: FollowerHooks): { start(): void; stop(): void };
  onTerminate(handler: () => void): void;
  homeDir(): string;
  now(): Date;
  log(line: string): void;
};

const contextText = (files: readonly { path: string; content: string }[]): string | undefined =>
  files.length === 0 ? undefined : files.map((f) => `## ${f.path}\n${f.content}`).join("\n\n");

/** Runs one Lead's or Manager's Session (contract §6.13). Resolves only on terminate (143), a closed conversation (1) or a failure (1); a `usage` error is thrown before any effect. */
export async function runHost(config: Config, deps: Deps): Promise<number> {
  const model = deps.resolveModel(config.model);
  const failed = (what: string, e: unknown) => deps.log(`${what}: ${e instanceof Error ? e.message : String(e)}`);

  const shown = await deps.cpd(["session", "show", config.session]);
  const session = parseSession(shown);
  if (session.role !== "lead" && session.role !== "manager") throw new CpdError("usage", `session ${config.session} has role ${session.role}: the host runs a lead or a manager only`);

  let follower: { start(): void; stop(): void } | undefined;
  let end!: (code: number) => void;
  const ended = new Promise<number>((resolve) => {
    end = resolve;
  });
  deps.onTerminate(() => end(143));

  const reporter = new Reporter(
    async (w: Wanted) => {
      await deps.cpd(stateArgs(w, config.session));
    },
    (e) => failed("session state", e),
  );

  const start = async (): Promise<void> => {
    const policy = policySection(await deps.cpd(["policy", "show"]));
    const extension = hostExtension({ contextFiles: contextText(deps.contextFiles(config.cwd)), policy, zone: zoneLine(deps.now()) }, { cwd: config.cwd, home: deps.homeDir() });
    const conversation = await deps.open({ file: storePath(config.home, config.session), cwd: config.cwd, model, extension });
    conversation.closed.then(
      (reason) => {
        failed("the conversation closed", reason);
        end(1);
      },
      (e) => {
        failed("the conversation closed", e);
        end(1);
      },
    );
    // §3.6 allows `stopped -> idle`, never `stopped -> working`.
    if ((shown.session as { state?: unknown }).state === "stopped") reporter.want({ state: "idle" });

    let cursor = cursorFrom(session.registeredEvent, await conversation.entries(), config.session);
    let pending = 0;
    const seen = new Set<string>();
    const names = new Map<string, Promise<string | null>>();
    const nameOf = (id: string): Promise<string | null> => {
      let name = names.get(id);
      if (name === undefined) {
        name = deps.cpd(["session", "show", id]).then((s) => parseSession(s).name, (e) => {
          failed(`the sender ${id}`, e);
          return null;
        });
        names.set(id, name);
      }
      return name;
    };
    const submit = async (input: Input): Promise<void> => {
      pending++;
      reporter.want({ state: "working" });
      try {
        const submission = await conversation.submit(input);
        void submission.wait().then(
          () => {
            if (--pending === 0) reporter.want({ state: "idle" });
          },
          (e) => {
            failed("a submission", e);
            end(1);
          },
        );
      } catch (e) {
        pending--;
        throw e;
      }
    };
    follower = deps.follow?.({
      before: async () => {},
      onRunStart: () => {},
      onLine: (text) => {
        const line = parseLine(text);
        if (line.type !== "event") {
          failed("follower line", line.type === "failure" ? line.error : new CpdError("bad_output", line.type));
          return false;
        }
        const event = line.event;
        cursor = Math.max(cursor, event.id);
        if (seen.has(`event:${event.id}`)) return true;
        seen.add(`event:${event.id}`);
        const fail = (e: unknown) => {
          failed("an event", e);
          end(1);
        };
        // Submitted synchronously unless a sender's name must be read first: a lookup is the only thing that waits.
        const from = event.kind === "session-message" && event.session === config.session ? event.data.from : null;
        if (typeof from === "string") {
          nameOf(from)
            .then((name) => {
              const input = inputOf(event, config.session, session, name);
              return input && submit(input);
            })
            .catch(fail);
        } else {
          const input = inputOf(event, config.session, session);
          if (input) submit(input).catch(fail);
        }
        return true;
      },
      onFailure: (e) => failed("follower", e),
      cursor: () => cursor,
    });
    follower?.start();
  };
  start().catch((e) => {
    failed("the run", e);
    end(1);
  });

  const code = await ended;
  follower?.stop();
  await reporter.close({ state: "stopped" });
  return code;
}
