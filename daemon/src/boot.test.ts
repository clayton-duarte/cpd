import { describe, expect, it, afterEach } from "vitest";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// Boot smoke test: proves the daemon actually starts under Node's strip-only
// type-stripping mode (`--experimental-strip-types`), not just that it
// typechecks or passes unit tests. A TS construct that *emits* code (parameter
// properties, enums, decorators, namespaces) typechecks fine but is a hard
// SyntaxError at process start under strip-only mode, and no other gate here
// catches that.

describe("daemon boot", () => {
  let child: ChildProcess | undefined;
  let dir: string | undefined;

  afterEach(() => {
    if (child && !child.killed) child.kill();
    if (dir) rmSync(dir, { recursive: true, force: true });
    child = undefined;
    dir = undefined;
  });

  it("starts under --experimental-strip-types and answers GET /api/health", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-boot-test-"));
    const port = 20000 + Math.floor(Math.random() * 10000);
    const dbPath = join(dir, "test.sqlite");

    child = spawn(
      process.execPath,
      ["--experimental-strip-types", join(import.meta.dirname, "index.ts")],
      {
        env: { ...process.env, CPD_DAEMON_PORT: String(port), CPD_DB: dbPath },
        stdio: ["ignore", "pipe", "pipe"],
      },
    );

    let stderr = "";
    child.stderr?.on("data", (chunk) => {
      stderr += chunk.toString();
    });

    const deadline = Date.now() + 15_000;
    let ok = false;
    let lastError: unknown;
    while (Date.now() < deadline) {
      if (child.exitCode !== null) {
        throw new Error(`daemon exited early (code ${child.exitCode}):\n${stderr}`);
      }
      try {
        const res = await fetch(`http://localhost:${port}/api/health`);
        const body = (await res.json()) as { ok?: boolean; pid?: number; db?: string };
        if (res.ok && body.ok === true) {
          ok = true;
          expect(body.pid).toBe(child.pid);
          expect(body.db).toBe(dbPath);
          break;
        }
      } catch (err) {
        lastError = err;
      }
      await new Promise((resolve) => setTimeout(resolve, 150));
    }

    if (!ok) {
      throw new Error(
        `daemon never answered /api/health within 15s (stderr:\n${stderr})`,
        { cause: lastError },
      );
    }

    expect(ok).toBe(true);
  }, 20_000);
});
