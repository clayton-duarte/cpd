// Preview drift report/reject: thin wiring; every decision lives in drift.ts.
import { execFile } from "node:child_process";
import { statSync } from "node:fs";
import { readDrift, rejectDrift } from "../web/src/electron/drift.ts";

const usage = "usage: preview-drift.mjs status|reject [--worktree <dir>]";
const [verb, ...rest] = process.argv.slice(2);
let root = process.env.CPD_CHECKOUT;
if (rest.length === 2 && rest[0] === "--worktree") root = rest[1];
else if (rest.length !== 0) root = undefined;
if ((verb !== "status" && verb !== "reject") || !root) {
  console.error(usage);
  process.exit(2);
}

const exec = (file, args, o) =>
  new Promise((resolve, reject) => {
    execFile(file, args, { cwd: o.cwd, env: { ...process.env, ...o.env }, maxBuffer: 64 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (err && typeof err.code !== "number") reject(err);
      else resolve({ code: err ? err.code : 0, stdout, stderr });
    });
  });
const mtimeMs = (p) => {
  try {
    return statSync(p).mtimeMs;
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
};

try {
  const result = verb === "status" ? await readDrift({ exec, mtimeMs, now: () => Date.now() }, root) : await rejectDrift({ exec, now: () => Date.now() }, root);
  console.log(JSON.stringify(result));
} catch (error) {
  console.error(`preview-drift: ${error?.message ?? error}`);
  process.exit(1);
}
