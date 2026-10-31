import type { Extension } from "@earendil-works/pi-durable";
import type { Conversation } from "../durable-worker/run.ts";
import { CpdError } from "../lib/cpd.ts";
import { policySection } from "../lib/policy.ts";
import { parseSession } from "../lib/run.ts";
import { Reporter, stateArgs, type Wanted } from "../lib/state.ts";
import { zoneLine } from "../lib/time.ts";
import { storePath, type Config } from "./config.ts";
import { hostExtension } from "./extension.ts";

type Model = { provider: string; modelId: string; thinkingLevel?: string };

export type Deps = {
  cpd(args: readonly string[]): Promise<Record<string, unknown>>;
  resolveModel(pattern: string): Model;
  contextFiles(cwd: string): readonly { path: string; content: string }[];
  open(o: { file: string; cwd: string; model: Model; extension: Extension }): Promise<Conversation>;
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
  };
  start().catch((e) => {
    failed("the run", e);
    end(1);
  });

  const code = await ended;
  await reporter.close({ state: "stopped" });
  return code;
}
