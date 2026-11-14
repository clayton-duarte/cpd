// Like argv-contract.test.ts, exercise the built CLI at the WebUI/CLI boundary.
// Rust testsel tests stay in-process on FakeRunner; no process-debt exception is needed.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, expect, it } from "vitest";

const root = fileURLToPath(new URL("../../../../", import.meta.url));
const binary = process.env["CPD_BIN"] ?? join(root, "target/debug/cpd");
let directory: string | undefined;
let file = "";
beforeAll(async () => {
  if (!existsSync(binary)) throw new Error(`build the cpd CLI first or set CPD_BIN: ${binary}`);
  directory = await mkdtemp(join(root, "pi/test/qa-named-node-"));
  const path = join(directory, "execution.test.ts");
  file = relative(root, path);
  await writeFile(path, `import test from "node:test";
test("passed probe",()=>{});
test("second passed probe",()=>{});
test("skipped probe",{skip:true},()=>{throw Error("must not run");});
test("todo probe",{todo:true},()=>{});
if(false)test("absent probe",()=>{});
test("failed probe",()=>{throw Error("expected failure");});
test("a.c (1+1) [x]",()=>{});
`);
});
afterAll(async () => { if (directory !== undefined) await rm(directory, { recursive: true, force: true }); });

it.each([
  { titles: ["passed probe"], status: 0 },
  { titles: ["skipped probe"], status: 1 },
  { titles: ["todo probe"], status: 1 },
  { titles: ["absent probe"], status: 1 },
  { titles: ["failed probe"], status: 1 },
  { titles: ["passed probe", "second passed probe"], status: 0 },
  { titles: ["passed probe", "skipped probe"], status: 1 },
  { titles: ["a.c (1+1) [x]"], status: 0 },
])("the real CLI verifies execution of $titles", ({ titles, status }) => {
  const reply = spawnSync(binary, ["test", "run", ...titles.map((title) => `${file}::${title}`)], {
    cwd: root, encoding: "utf8", timeout: 10_000,
    env: { ...process.env, CPD_HOME: join(directory!, "home") },
  });
  expect(reply.error).toBeUndefined();
  expect(reply.status, reply.stdout + reply.stderr).toBe(status);
});
