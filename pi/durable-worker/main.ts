import { execFile } from "node:child_process";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";
import { getAgentDir, loadProjectContextFiles, ModelRuntime, resolveCliModel } from "@earendil-works/pi-coding-agent";
import { CodemodeSandbox } from "@earendil-works/pi-codemode";
import { createCodemodeExtension } from "@earendil-works/pi-durable/codemode";
import { callCpd, CpdError } from "../lib/cpd.ts";
import { DEFAULT_TIMING, Follower } from "../lib/follower.ts";
import { parseConfig } from "./config.ts";
import { openConversation } from "./harness.ts";
import { piMcpPort } from "./pi-mcp.ts";
import { writeReport } from "./report.ts";
import { runWorker, type Deps } from "./run.ts";

const exec = (command: string, args: string[], options: { timeout?: number; signal?: AbortSignal } = {}) =>
  new Promise<{ stdout: string; stderr: string; code: number; killed: boolean }>((resolve) => {
    execFile(command, args, { timeout: options.timeout, signal: options.signal, encoding: "utf8" }, (error, stdout, stderr) => {
      const e = error as (NodeJS.ErrnoException & { killed?: boolean }) | null;
      if (e && typeof e.code === "string" && !e.killed && e.name !== "AbortError") throw e;
      resolve({ stdout, stderr, code: e ? (typeof e.code === "number" ? e.code : 1) : 0, killed: Boolean(e?.killed) || e?.name === "AbortError" });
    });
  });

const sandbox = new CodemodeSandbox();

try {
  const config = parseConfig(process.argv.slice(2), process.env, process.cwd());
  const models = await ModelRuntime.create({ refreshOnCreate: false });
  const deps: Deps = {
    cpd: (args) => callCpd(exec as never, [...args], { timeoutMs: 30_000 }),
    resolveModel: (pattern) => {
      const r = resolveCliModel({ cliModel: pattern, modelRuntime: models });
      if (r.error !== undefined || r.model === undefined) throw new CpdError("usage", `unusable model ${pattern}: ${r.error ?? "not found"}`);
      return { provider: r.model.provider, modelId: r.model.id, ...(r.thinkingLevel === undefined ? {} : { thinkingLevel: r.thinkingLevel }) };
    },
    contextFiles: (cwd) => loadProjectContextFiles({ cwd, agentDir: getAgentDir() }),
    open: (o) => openConversation(models as never, o),
    follow: (hooks) => new Follower(config.session, hooks, DEFAULT_TIMING),
    writeReport,
    onTerminate: (handler) => {
      process.once("SIGTERM", handler);
      process.once("SIGINT", handler);
    },
    codemode: (namespaces) => createCodemodeExtension({ sandbox, namespaces: [...namespaces] }),
    mcp: piMcpPort(fileURLToPath(import.meta.resolve("@earendil-works/pi-coding-agent")), config.cwd),
    homeDir: homedir,
    now: () => new Date(),
    log: (line) => console.error(line),
  };
  process.exit(await runWorker(config, deps));
} catch (e) {
  if (e instanceof CpdError && e.code === "usage") {
    console.error(`${e.code}: ${e.message}`);
    process.exit(2);
  }
  throw e;
}
