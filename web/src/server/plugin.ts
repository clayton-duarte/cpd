// STUB (red-test Job): the implementation Job replaces the bodies. Boundary types are declared loosely here only until their owning modules exist.
import type { Plugin } from "vite";
import type { Runner } from "../protocol/domain.ts";
import type { Scheduler } from "../protocol/runtime.ts";
import type { ServerMessage } from "../protocol/wire.ts";
import type { CockpitService, HubConnection, SessionBackend } from "./hub/connection.ts";
import type { Registry } from "./hub/registry.ts";

export interface PluginBoundaries {
  cpdExec: { run(args: readonly string[]): Promise<{ stdout: string; stderr: string; exitCode: number }> };
  follow: { start(args: readonly string[]): unknown };
  socketFactory: unknown;
  fs: unknown;
  watch: unknown;
  git: { branch(cwd: string): Promise<string | null> };
  scheduler: Scheduler;
  cpdHome: string;
}
export interface Services {
  cockpit: CockpitService;
  registry: Registry;
  backends: Record<Runner, SessionBackend>;
  newConnection(send: (m: ServerMessage) => void): HubConnection;
}
export function createServices(_b: PluginBoundaries): Services {
  throw new Error("not implemented");
}
export function realBoundaries(_env: NodeJS.ProcessEnv): PluginBoundaries {
  throw new Error("not implemented");
}
export function cpdWebPlugin(_options?: { boundaries?: PluginBoundaries }): Plugin {
  throw new Error("not implemented");
}
