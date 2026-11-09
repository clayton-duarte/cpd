import { mkdtempSync, readFileSync, rmSync, statSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { writeWebuiToken } from "./webui-token.ts";

describe("writeWebuiToken", () => {
  let home: string;
  beforeEach(() => {
    home = mkdtempSync(join(tmpdir(), "webui-token-"));
  });
  afterEach(() => rmSync(home, { recursive: true, force: true }));

  it("writes 32 random bytes as hex to run/webui-token at mode 0600, new on every boot", () => {
    const first = writeWebuiToken(home);
    const file = join(home, "run", "webui-token");
    expect(first.token).toMatch(/^[0-9a-f]{64}$/);
    expect(readFileSync(file, "utf8")).toBe(first.token);
    expect(statSync(file).mode & 0o777).toBe(0o600);
    const second = writeWebuiToken(home);
    expect(second.token).not.toBe(first.token);
    expect(readFileSync(file, "utf8")).toBe(second.token);
  });

  it("replaces a file of an earlier start, whatever its mode", () => {
    mkdirSync(join(home, "run"));
    writeFileSync(join(home, "run", "webui-token"), "old", { mode: 0o644 });
    writeWebuiToken(home);
    expect(statSync(join(home, "run", "webui-token")).mode & 0o777).toBe(0o600);
  });

  it("removes the file when it stops", () => {
    const issued = writeWebuiToken(home);
    issued.remove();
    expect(existsSync(join(home, "run", "webui-token"))).toBe(false);
    issued.remove();
  });
});
