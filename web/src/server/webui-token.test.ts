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

  it("writes 32 random bytes as hex to run/webui-tokens/<pid>.token at mode 0600, new on every boot", () => {
    const first = writeWebuiToken(home);
    const tokensDir = join(home, "run", "webui-tokens");
    const file = join(tokensDir, `${first.pid}.token`);
    expect(first.token).toMatch(/^[0-9a-f]{64}$/);
    expect(readFileSync(file, "utf8")).toBe(first.token);
    expect(statSync(file).mode & 0o777).toBe(0o600);
    const second = writeWebuiToken(home);
    expect(second.token).not.toBe(first.token);
    expect(readFileSync(join(tokensDir, `${second.pid}.token`), "utf8")).toBe(second.token);
  });

  it("replaces a file of an earlier start of the same pid, whatever its mode", () => {
    const tokensDir = join(home, "run", "webui-tokens");
    mkdirSync(tokensDir, { recursive: true });
    writeFileSync(join(tokensDir, "12345.token"), "old", { mode: 0o644 });
    writeWebuiToken(home, 12345);
    expect(statSync(join(tokensDir, "12345.token")).mode & 0o777).toBe(0o600);
  });

  it("removes the file when it stops", () => {
    const issued = writeWebuiToken(home);
    const tokensDir = join(home, "run", "webui-tokens");
    const file = join(tokensDir, `${issued.pid}.token`);
    expect(existsSync(file)).toBe(true);
    issued.remove();
    expect(existsSync(file)).toBe(false);
    issued.remove();
  });

  it("webui-token multi-server survival", () => {
    // b13: two servers run concurrently, each writing its own token file
    // when one server quits and removes its token, the other server's token remains
    const s1 = writeWebuiToken(home);
    const s2 = writeWebuiToken(home);
    expect(s1.token).not.toBe(s2.token);

    // In single-file implementation, s2 replaced s1's file at run/webui-token.
    // When s1 stops, in single-file it removes run/webui-token, destroying s2's token.
    s1.remove();

    // Under per-server isolation (contract §6.16, web/CONTRACT.md §27):
    // Each server has its own token file in run/webui-tokens/<pid>.token.
    // s1's removal must not remove s2's file, and s2's token must still exist.
    const tokensDir = join(home, "run", "webui-tokens");
    expect(existsSync(tokensDir)).toBe(true);
    expect(existsSync(join(tokensDir, `${s2.pid}.token`))).toBe(true);
    expect(readFileSync(join(tokensDir, `${s2.pid}.token`), "utf8")).toBe(s2.token);
    s2.remove();
  });

  it("webui-token per-server file isolation", () => {
    // contract §6.16: each server writes $CPD_HOME/run/webui-tokens/<pid>.token
    // and neither writes nor removes another server's file or the deprecated run/webui-token.
    const issued = writeWebuiToken(home);
    const tokensDir = join(home, "run", "webui-tokens");
    expect(existsSync(tokensDir)).toBe(true);
    expect(existsSync(join(home, "run", "webui-token"))).toBe(false);
    issued.remove();
  });
});
