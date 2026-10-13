import { mock, test } from "node:test";
import { Socket } from "node:net";
import assert from "node:assert/strict";
import { z } from "zod";
import { run, runJson } from "../../src/lib/exec.ts";
import { fakeBin, tmpDir } from "../helpers/index.ts";

const opts = () => ({ cwd: tmpDir(), env: { PATH: process.env.PATH } });

test("run captures stdout, stderr and exit code", async () => {
  const r = await run("sh", ["-c", "echo out; echo err >&2; exit 7"], opts());
  assert.deepEqual(r, { code: 7, signal: null, stdout: "out\n", stderr: "err\n" });
});

test("run reports the signal when the process is killed by one", async () => {
  const r = await run("sh", ["-c", "kill -TERM $$"], opts());
  assert.equal(r.code, null);
  assert.equal(r.signal, "SIGTERM");
});

test("runJson names the signal when the process is killed", async () => {
  await assert.rejects(
    runJson("sh", ["-c", "kill -TERM $$"], opts(), z.any()),
    (e: any) => e.code === "bad_tool_output" && e.details.signal === "SIGTERM" && e.message.includes("SIGTERM"),
  );
});

test("run passes input on stdin, and /dev/null otherwise", async () => {
  assert.equal((await run("cat", [], { ...opts(), input: "hi" })).stdout, "hi");
  assert.equal((await run("cat", [], opts())).stdout, "");
});

test("run times out with code exec_timeout", async () => {
  await assert.rejects(run("sleep", ["5"], { ...opts(), timeoutMs: 100 }), (e: any) => e.code === "exec_timeout" && e.exitCode === 5);
});

test("runJson parses and validates", async () => {
  const b = fakeBin("tool", `echo '{"a":1}'`);
  const v = await runJson(b.path, [], opts(), z.object({ a: z.number() }));
  assert.deepEqual(v, { a: 1 });
});

test("runJson fails with bad_tool_output on invalid JSON, with tails", async () => {
  await assert.rejects(
    runJson("sh", ["-c", "echo notjson; echo oops >&2"], opts(), z.any()),
    (e: any) => e.code === "bad_tool_output" && e.details.stdout.includes("notjson") && e.details.stderr.includes("oops"),
  );
});

test("runJson fails with bad_tool_output on schema mismatch and non-zero exit", async () => {
  await assert.rejects(runJson("sh", ["-c", "echo '{}'"], opts(), z.object({ a: z.number() })), (e: any) => e.code === "bad_tool_output");
  await assert.rejects(runJson("sh", ["-c", "echo '{}'; exit 3"], opts(), z.any()), (e: any) => e.code === "bad_tool_output");
});

test("run ignores EPIPE on stdin (the child exits without reading its input)", async () => {
  const r = await run("sh", ["-c", "exit 3"], { ...opts(), input: "x".repeat(4 * 1024 * 1024) });
  assert.equal(r.code, 3);
});

test("run fails loudly on any other stdin error", async () => {
  const end = Socket.prototype.end;
  const m = mock.method(Socket.prototype, "end", function (this: Socket, ...a: unknown[]) {
    this.emit("error", Object.assign(new Error("boom"), { code: "EIO" }));
    return (end as (...x: unknown[]) => Socket).apply(this, a);
  });
  try {
    await assert.rejects(
      run("cat", [], { ...opts(), input: "hi" }),
      (e: any) => e.code === "stdin_failed" && e.message.includes("boom"),
    );
  } finally {
    m.mock.restore();
  }
});
