import type { Extension } from "@earendil-works/pi-durable";
import type { FollowerHooks } from "../lib/follower.ts";
import type { Config } from "./config.ts";
import type { Input } from "./deliver.ts";
import type { EndReport, Entry } from "./report.ts";

export type Submission = { wait(): Promise<"done" | "unanswered"> };
export type Conversation = { submit(input: Input): Promise<Submission>; entries(): Promise<readonly Entry[]>; closed: Promise<unknown> };

type Model = { provider: string; modelId: string; thinkingLevel?: string };

export type Deps = {
  cpd(args: readonly string[]): Promise<Record<string, unknown>>;
  resolveModel(pattern: string): Model;
  contextFiles(cwd: string): readonly { path: string; content: string }[];
  open(o: { file: string; cwd: string; model: Model; extension: Extension }): Promise<Conversation>;
  follow(hooks: FollowerHooks): { start(): void; stop(): void };
  writeReport(file: string, report: EndReport): Promise<void>;
  onTerminate(handler: () => void): void;
  homeDir(): string;
  now(): Date;
  log(line: string): void;
};

export async function runWorker(_config: Config, _deps: Deps): Promise<number> {
  throw new Error("not implemented");
}
