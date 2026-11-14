import { mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { Server } from "node:http";

function atomic(file: string, text: string): void {
  const temporary = `${file}.${process.pid}.tmp`;
  writeFileSync(temporary, text, { mode: 0o600 });
  renameSync(temporary, file);
}
function read(file: string): string | null {
  try { return readFileSync(file, "utf8").trim(); } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  }
}

/** Only a listening server publishes its PID. Staging never overwrites the current listener. */
export async function publishRuntime(server: Server, home: string, host: string, port: number, standby: boolean): Promise<void> {
  const run = join(home, "run");
  mkdirSync(run, { recursive: true });
  const pidFile = join(run, "webui.pid");
  const prepared = join(run, "webui-prepared.json");
  const activation = join(run, "webui-activate.pid");
  const mark = (phase: "staged" | "active") => atomic(prepared, JSON.stringify({ pid: process.pid, ready: true, phase }));
  const publish = () => atomic(pidFile, String(process.pid));
  process.once("exit", () => {
    if (read(pidFile) === String(process.pid)) rmSync(pidFile, { force: true });
  });
  if (!standby) { publish(); return; }
  mark("staged");
  // Activation is an owner-only filesystem request. No network endpoint can signal a process.
  const deadline = Date.now() + 180_000;
  const wait = setInterval(() => {
    if (read(activation) === String(process.pid)) {
      clearInterval(wait);
      rmSync(activation, { force: true });
      server.close(() => {
        const until = Date.now() + 10_000;
        const listen = () => server.listen(port, host, () => { publish(); mark("active"); });
        const failed = (error: NodeJS.ErrnoException) => {
          if (error.code === "EADDRINUSE" && Date.now() < until) setTimeout(listen, 100);
          else { console.error("cpd-web: prepared listener activation failed:", error.message); process.exit(1); }
        };
        server.on("error", failed);
        server.once("listening", () => server.off("error", failed));
        listen();
      });
    } else if (Date.now() >= deadline) {
      clearInterval(wait);
      console.error("cpd-web: prepared WebUI expired without activation");
      process.exit(1);
    }
  }, 100);
}
