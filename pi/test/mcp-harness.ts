import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createAgentSession, DefaultResourceLoader, type ExtensionFactory, type McpExtensionOptions, SessionManager, SettingsManager } from "@earendil-works/pi-coding-agent";
import { notify } from "./fake.ts";

/** A real pi 1.3.0 SDK session with an inline extension factory set (docs/sdk.md "Codemode and MCP"), in a temporary agent directory and cwd. No network, no model call. */

type CreateTransport = NonNullable<McpExtensionOptions["createTransport"]>;
export type TransportCall = { name: string; config: unknown; cwd: string };

export type Harness = {
  agentDir: string;
  cwd: string;
  /** The servers `createTransport` was asked for, in order. */
  transports: TransportCall[];
  /** A `createTransport` that records its entry and never connects (`start` fails at once). Pass it to `mcpMount({ createTransport })`. */
  createTransport: CreateTransport;
  /** Runs `body` with `PI_CODING_AGENT_DIR` set to the temporary agent directory and restores it, then removes the temporary directories. */
  run(body: (h: Harness) => Promise<void>): Promise<void>;
  /** Builds the session with `factories`, binds the extensions (`session_start`) (pi connects registered servers in the background: wait for them with `waitFor`); the session is disposed by `run`. */
  session(factories: ExtensionFactory[], options?: { trusted?: boolean }): Promise<void>;
};

export function harness(): Harness {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "mcp-mount-")));
  const agentDir = join(root, "agent");
  const cwd = join(root, "project");
  mkdirSync(agentDir);
  mkdirSync(cwd);
  const transports: TransportCall[] = [];
  const disposers: (() => void)[] = [];
  const createTransport: CreateTransport = (entry, dir) => {
    transports.push({ name: entry.name, config: entry.config, cwd: dir });
    notify();
    return {
      start: () => Promise.reject(new Error("fake transport: never connects")),
      send: () => Promise.reject(new Error("fake transport: never connects")),
      close: () => Promise.resolve(),
      onMessage: () => () => {},
      onError: () => () => {},
      onClose: () => () => {},
    };
  };
  const h: Harness = {
    agentDir,
    cwd,
    transports,
    createTransport,
    async run(body) {
      const saved = process.env.PI_CODING_AGENT_DIR;
      process.env.PI_CODING_AGENT_DIR = agentDir;
      try {
        await body(h);
      } finally {
        for (const d of disposers) d();
        if (saved === undefined) delete process.env.PI_CODING_AGENT_DIR;
        else process.env.PI_CODING_AGENT_DIR = saved;
        rmSync(root, { recursive: true, force: true });
      }
    },
    async session(factories, options = {}) {
      const resourceLoader = new DefaultResourceLoader({
        cwd,
        agentDir,
        settingsManager: SettingsManager.inMemory(),
        noExtensions: true, // discovery: only the inline factories load
        extensionFactories: factories,
      });
      await resourceLoader.reload({ resolveProjectTrust: async () => options.trusted === true });
      const { session } = await createAgentSession({ cwd, agentDir, resourceLoader, settingsManager: SettingsManager.inMemory(), sessionManager: SessionManager.inMemory() });
      disposers.push(() => session.dispose());
      await session.bindExtensions({});
    },
  };
  return h;
}

export const writeMcpJson = (dir: string, servers: Record<string, unknown>) => {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "mcp.json"), JSON.stringify({ mcpServers: servers }));
};
