import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { homedir } from "node:os";
import { pathToFileURL } from "node:url";
import { getAgentDir, loadProjectContextFiles, ModelRuntime, resolveCliModel } from "@earendil-works/pi-coding-agent";
import { getSupportedThinkingLevels } from "@earendil-works/pi-ai";
import { callCpd, CpdError } from "../lib/cpd.ts";
import { validatePackage } from "../lib/reload.ts";
import { openConversation } from "../durable-worker/harness.ts";
import { DEFAULT_TIMING, Follower } from "../lib/follower.ts";
import { guardSocket, listenOn } from "./net.ts";
import { parseConfig, type Config } from "./config.ts";
import { runHost, type Deps } from "./run.ts";

const exec = (command: string, args: string[], options: { timeout?: number; signal?: AbortSignal } = {}) =>
  new Promise<{ stdout: string; stderr: string; code: number; killed: boolean }>((resolve) => {
    execFile(command, args, { timeout: options.timeout, signal: options.signal, encoding: "utf8" }, (error, stdout, stderr) => {
      const e = error as (NodeJS.ErrnoException & { killed?: boolean }) | null;
      if (e && typeof e.code === "string" && !e.killed && e.name !== "AbortError") throw e;
      resolve({ stdout, stderr, code: e ? (typeof e.code === "number" ? e.code : 1) : 0, killed: Boolean(e?.killed) || e?.name === "AbortError" });
    });
  });

/** The slice of pi's ModelRuntime the host deps use (tests give a fake: no network, no model). */
export type HostModels = Pick<ModelRuntime, "getAvailable"> & {
  getModelOfType(type: "classifier", providerId: string, modelId: string): unknown;
  classify(model: unknown, context: unknown, options?: { signal?: AbortSignal }): Promise<unknown>;
};

/** The `Deps` of the host process. STUB (hostfix-tests-a1): builds the old Deps, without `classify`. */
export function hostDeps(config: Config, models: HostModels): Deps {
  const real = models as unknown as ModelRuntime;
  return {
    cpd: (args) => callCpd(exec as never, [...args], { timeoutMs: 30_000 }),
    resolveModel: (pattern) => {
      const r = resolveCliModel({ cliModel: pattern, modelRuntime: real });
      if (r.error !== undefined || r.model === undefined) throw new CpdError("usage", `unusable model ${pattern}: ${r.error ?? "not found"}`);
      return { provider: r.model.provider, modelId: r.model.id, ...(r.thinkingLevel === undefined ? {} : { thinkingLevel: r.thinkingLevel }) };
    },
    models: async () =>
      (await models.getAvailable()).map((m) => ({
        provider: m.provider,
        model_id: m.id,
        name: m.name,
        thinking_levels: getSupportedThinkingLevels(m),
        context_window: m.contextWindow,
      })),
    contextFiles: (cwd) => loadProjectContextFiles({ cwd, agentDir: getAgentDir() }),
    validatePackage: (dir) => validatePackage(dir),
    open: (o) => openConversation(real as never, o),
    follow: (hooks) => new Follower(config.session, hooks, DEFAULT_TIMING),
    guard: guardSocket,
    listen: listenOn,
    newEpoch: () => randomUUID(),
    onTerminate: (handler) => {
      process.once("SIGTERM", handler);
      process.once("SIGINT", handler);
    },
    homeDir: homedir,
    now: () => new Date(),
    log: (line) => console.error(line),
  };
}

// Only the process entry starts a host: a test imports `hostDeps`.
if (process.argv[1] !== undefined && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try {
    const config = parseConfig(process.argv.slice(2), process.env, process.cwd());
    const models = await ModelRuntime.create({ refreshOnCreate: false });
    process.exit(await runHost(config, hostDeps(config, models as never)));
  } catch (e) {
    if (e instanceof CpdError && e.code === "usage") {
      console.error(`${e.code}: ${e.message}`);
      process.exit(2);
    }
    throw e;
  }
}
