import { test } from "node:test";
import assert from "node:assert/strict";
import { run } from "../../src/lib/exec.ts";
import { fakeBin, sandboxEnv, tmpDir } from "./index.ts";

test("fakeBin records argv as JSON lines and runs its script", async () => {
  const b = fakeBin("mytool", "echo hello; exit 3");
  const env = sandboxEnv({}, [b.dir]);
  const r1 = await run("mytool", ["a b", "--c=\"d\""], { cwd: tmpDir(), env });
  await run("mytool", [], { cwd: tmpDir(), env });
  assert.equal(r1.code, 3);
  assert.equal(r1.stdout, "hello\n");
  assert.deepEqual(b.calls(), [["a b", "--c=\"d\""], []]);
});

test("sandboxEnv sets HOME and CPD_HOME under temp, keeps PATH", () => {
  const e = sandboxEnv();
  assert.equal(e.PATH, process.env.PATH);
  assert.ok(e.HOME !== process.env.HOME);
  assert.ok(e.CPD_HOME!.startsWith(e.HOME!.replace(/home$/, "")));
});
