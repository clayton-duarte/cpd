import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin, type PluginOption } from "vite";
import { createLiveHandler, type LiveDeps, type LiveSession, type InboxItem, type LivePlan } from "./src/api/live-server.ts";

// The web client dev/build config (docs/research/web-client-api.md §6, A2): React + the
// Tailwind v4 plugin, and the dev server on port 5173 against the typed mock. With
// CPD_WEB_LIVE=1 it also mounts the dev live adapter (web/README.md); without it, nothing new.

/** The cpd binary: `$CPD_BIN`, else this repo's built binary, else `cpd` on PATH. */
function cpdBin(): string {
  if (process.env.CPD_BIN) return process.env.CPD_BIN;
  const built = fileURLToPath(new URL("../target/debug/cpd", import.meta.url));
  return existsSync(built) ? built : "cpd";
}

/** Runs one cpd command and resolves its stdout; a non-zero exit rejects. */
function runCpd(args: readonly string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(cpdBin(), [...args], { maxBuffer: 64 * 1024 * 1024 }, (error, stdout) => {
      if (error) reject(error);
      else resolve(stdout);
    });
  });
}

/** One `session list` row (only the fields realDeps reads). */
interface SessionRow {
  id?: unknown;
  role?: unknown;
  runner?: unknown;
  name?: unknown;
  state?: unknown;
  title?: unknown;
  context?: unknown;
  file?: unknown;
}

/** One `plan list` row (only the fields realDeps reads). */
interface PlanListRow {
  id?: unknown;
  context?: unknown;
  title?: unknown;
  phase?: unknown;
  lead_session?: unknown;
  archived_at?: unknown;
  counts?: unknown;
}

/** The six Job counts `cpd plan list` prints, coerced to numbers with a missing one at zero. */
function planCounts(value: unknown): LivePlan["counts"] {
  const counts = (value ?? {}) as Record<string, unknown>;
  const at = (key: string): number => (typeof counts[key] === "number" ? (counts[key] as number) : 0);
  return {
    draft: at("draft"),
    queued: at("queued"),
    running: at("running"),
    merged: at("merged"),
    failed: at("failed"),
    cancelled: at("cancelled"),
  };
}

/** Builds the real `LiveDeps`: it runs the cpd CLI and reads the pi session file it names. */
function realDeps(): LiveDeps {
  // `session list` carries each Session's pi session file; the map saves a second call.
  const files = new Map<string, string>();

  const list = async (): Promise<readonly LiveSession[]> => {
    const parsed = JSON.parse(
      await runCpd(["session", "list", "--role", "lead", "--role", "manager"]),
    ) as { sessions?: SessionRow[] };
    const sessions: LiveSession[] = [];
    for (const row of parsed.sessions ?? []) {
      const runner = row.runner === "durable" ? "durable" : "pi";
      const id = String(row.id);
      if (typeof row.file === "string") files.set(id, row.file);
      const session: LiveSession = {
        id,
        role: row.role === "manager" ? "manager" : "lead",
        runner,
      };
      const title = typeof row.title === "string" ? row.title : row.name;
      if (typeof title === "string") session.title = title;
      if (typeof row.state === "string") session.state = row.state;
      if (typeof row.context === "string") session.context = row.context;
      if (typeof row.name === "string") session.name = row.name;
      sessions.push(session);
    }
    return sessions;
  };

  const piFile = async (id: string): Promise<string | undefined> => {
    const cached = files.get(id);
    if (cached !== undefined) return cached;
    const parsed = JSON.parse(await runCpd(["session", "show", id])) as {
      session?: { file?: unknown };
    };
    const file = parsed.session?.file;
    if (typeof file !== "string") return undefined;
    files.set(id, file);
    return file;
  };

  return {
    list,
    async readPi(id) {
      const file = await piFile(id);
      if (file === undefined) return undefined;
      try {
        return await readFile(file, "utf8");
      } catch {
        return undefined;
      }
    },
    async tail(id) {
      return await runCpd(["session", "tail", id]);
    },
    async send(id, text, unblocks) {
      await runCpd(["session", "send", id, text, "--unblocks", unblocks]);
    },
    async stat(id) {
      const file = await piFile(id);
      if (file === undefined) return undefined;
      try {
        return (await stat(file)).mtimeMs;
      } catch {
        return undefined;
      }
    },
    async inbox() {
      const parsed = JSON.parse(await runCpd(["inbox", "list"])) as { items?: readonly InboxItem[] };
      return parsed.items ?? [];
    },
    async plans() {
      // The CLI's own rows; an archived Plan is never drawn (web/README.md "Context tree rows").
      const parsed = JSON.parse(await runCpd(["plan", "list"])) as { plans?: readonly PlanListRow[] };
      const plans: LivePlan[] = [];
      for (const row of parsed.plans ?? []) {
        if (row.archived_at !== null && row.archived_at !== undefined) continue;
        plans.push({
          id: String(row.id),
          context: String(row.context ?? ""),
          title: String(row.title ?? ""),
          phase: String(row.phase ?? ""),
          lead_session: typeof row.lead_session === "string" ? row.lead_session : null,
          archived_at: null,
          counts: planCounts(row.counts),
        });
      }
      return plans;
    },
  };
}

/** Mounts the dev live adapter's `/__cpd/` middleware; only added with CPD_WEB_LIVE=1. */
function liveAdapterPlugin(): Plugin {
  return {
    name: "cpd-live-adapter",
    apply: "serve",
    configureServer(server) {
      const handler = createLiveHandler(realDeps());
      server.middlewares.use((req, res, next) => {
        handler(req, res).then(
          (matched) => {
            if (!matched) next();
          },
          (error: unknown) => next(error instanceof Error ? error : new Error(String(error))),
        );
      });
    },
  };
}

// The live adapter is opt-in: exactly CPD_WEB_LIVE=1 mounts it; anything else (including unset)
// leaves the existing dev and build flows exactly as they were.
const plugins: PluginOption[] = [react(), tailwindcss()];
if (process.env.CPD_WEB_LIVE === "1") plugins.push(liveAdapterPlugin());

export default defineConfig({
  plugins,
  server: {
    port: 5173,
    strictPort: true,
  },
});
