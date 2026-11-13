import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { mock, test } from "node:test";
import { webuiVerified } from "../../durable-host/webui-token.ts";

test("the process check treats EPERM as alive and ESRCH or other failures as unverified", async () => {
  const home = mkdtempSync(join(tmpdir(), "host-token-"));
  try {
    const dir = join(home, "run", "webui-tokens");
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "123.token"), "token");
    for (const [code, expected] of [["EPERM", true], ["ESRCH", false], ["EINVAL", false]] as const) {
      const kill = mock.method(process, "kill", () => { throw Object.assign(new Error("process check failed"), { code }); });
      try { assert.equal(await webuiVerified(home, "token"), expected); }
      finally { kill.mock.restore(); }
    }
  } finally { rmSync(home, { recursive: true, force: true }); }
});

test("per-server verification requires a nonempty matching token of a live PID and rereads on revocation", async () => {
  const home = mkdtempSync(join(tmpdir(), "host-token-"));
  const dir = join(home, "run", "webui-tokens");
  try {
    const alive = (pid: number) => pid === 11 || pid === 12;
    assert.equal(await webuiVerified(home, "a", alive), false);
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "11.token"), "alpha");
    writeFileSync(join(dir, "12.token"), "beta");
    writeFileSync(join(dir, "13.token"), "dead");
    writeFileSync(join(home, "run", "webui-token"), "legacy");
    for (const token of [undefined, "", "wrong", "dead", "legacy"])
      assert.equal(await webuiVerified(home, token, alive), false);
    assert.equal(await webuiVerified(home, "alpha", alive), true);
    assert.equal(await webuiVerified(home, "beta", alive), true);
    rmSync(join(dir, "11.token"));
    assert.equal(await webuiVerified(home, "alpha", alive), false);
    assert.equal(await webuiVerified(home, "beta", alive), true);
    writeFileSync(join(dir, "12.token"), "new");
    assert.equal(await webuiVerified(home, "beta", alive), false);
    assert.equal(await webuiVerified(home, "new", alive), true);
  } finally { rmSync(home, { recursive: true, force: true }); }
});

test("invalid PID names and unreadable entries cannot verify a token", async () => {
  const home = mkdtempSync(join(tmpdir(), "host-token-"));
  const dir = join(home, "run", "webui-tokens");
  try {
    mkdirSync(dir, { recursive: true });
    for (const file of ["0.token", "-1.token", "11junk.token", "999999999999999999999.token", "2147483648.token"])
      writeFileSync(join(dir, file), "invalid");
    mkdirSync(join(dir, "14.token"));
    writeFileSync(join(dir, "15.token"), "valid");
    const checked: number[] = [];
    const alive = (pid: number) => { checked.push(pid); return true; };
    assert.equal(await webuiVerified(home, "invalid", alive), false);
    assert.deepEqual(checked.sort((a, b) => a - b), [14, 15]);
    assert.equal(await webuiVerified(home, "valid", alive), true);
  } finally { rmSync(home, { recursive: true, force: true }); }
});
