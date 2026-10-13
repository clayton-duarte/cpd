import assert from "node:assert/strict";
import { chmodSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { rebaseInProgress } from "../../src/git/git.ts";
import { fakeBin, tmpDir } from "../helpers/index.ts";
import { createRepo, sandbox } from "./fixtures.ts";

sandbox();

test("rebaseInProgress is false when no rebase directory exists", async () => {
  assert.equal(await rebaseInProgress(await createRepo()), false);
});

test("rebaseInProgress rethrows anything but ENOENT (git-path inside a directory without permission: EACCES)", async () => {
  const locked = join(tmpDir(), "locked");
  mkdirSync(locked);
  chmodSync(locked, 0o000);
  const bin = fakeBin("git", `echo '${join(locked, "sub", "rebase-merge")}'`);
  const oldPath = process.env.PATH;
  process.env.PATH = `${bin.dir}:${oldPath}`;
  try {
    await assert.rejects(rebaseInProgress(tmpDir()), (e: NodeJS.ErrnoException) => e.code === "EACCES");
  } finally {
    process.env.PATH = oldPath;
    chmodSync(locked, 0o755);
  }
});
