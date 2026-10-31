import type { Extension } from "@earendil-works/pi-durable";
import type { Conversation } from "../durable-worker/run.ts";
import type { Config } from "./config.ts";

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

export async function runHost(_config: Config, _deps: Deps): Promise<number> {
  throw new Error("not implemented");
}
