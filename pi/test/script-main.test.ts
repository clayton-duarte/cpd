import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const piDir = realpathSync(join(here, ".."));
const modelsList = join(piDir, "lib", "models-list.ts");

function tsFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return e.name === "node_modules" ? [] : tsFiles(p);
    return e.name.endsWith(".ts") ? [p] : [];
  });
}

const forbidden = new RegExp(["process\\.argv\\[1\\]", "import\\.meta\\.(url|filename)\\s*[!=]=="].join("|"));

test("no source under pi/ decides main by comparing argv[1] with the module's url", () => {
  const self = fileURLToPath(import.meta.url);
  const bad = tsFiles(piDir)
    .filter((f) => f !== self && forbidden.test(readFileSync(f, "utf8")))
    .map((f) => relative(piDir, f));
  assert.deepEqual(bad, []);
});

test("every pi/lib script that runs at top level decides main with import.meta.main", () => {
  for (const f of ["models-list.ts", "slack-channel.ts"]) {
    assert.ok(readFileSync(join(piDir, "lib", f), "utf8").includes("if (import.meta.main)"), f);
  }
});

function runMain(script: string) {
  const env = { ...process.env };
  delete env.PI_CODING_AGENT_DIR;
  return spawnSync(process.execPath, [script], { encoding: "utf8", timeout: 30_000, env });
}

function assertFails(script: string) {
  const r = runMain(script);
  assert.equal(r.status, 1, r.stderr);
  const out = JSON.parse(r.stdout) as { ok: boolean; error: { code: string } };
  assert.equal(out.ok, false);
  assert.equal(out.error.code, "models_helper_failed");
}

test("models-list.ts reached through a symlink to its script still runs main", (t) => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "models-link-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const link = join(root, "models-list.ts");
  symlinkSync(modelsList, link);
  assertFails(link);
  assertFails(modelsList);
});

test("models-list.ts reached through a symlink to its directory still runs main", (t) => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "models-dirlink-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  symlinkSync(piDir, join(root, "pi"));
  assertFails(join(root, "pi", "lib", "models-list.ts"));
  assertFails(modelsList);
});

test("a module that only imports models-list.ts does not run main", (t) => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "models-import-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const script = join(root, "importer.ts");
  writeFileSync(script, `import ${JSON.stringify(pathToFileURL(modelsList).href)};\n`);
  const r = spawnSync(process.execPath, [script], { encoding: "utf8", timeout: 30_000 });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.stdout, "");
});
