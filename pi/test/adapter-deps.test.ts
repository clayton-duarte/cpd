import assert from "node:assert/strict";
import { readdirSync, readFileSync, realpathSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const piDir = realpathSync(join(here, ".."));
const root = join(piDir, "..");

function tsFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return e.name === "node_modules" || p === join(piDir, "test") ? [] : tsFiles(p);
    return e.name.endsWith(".ts") && !/\.test\.ts$/.test(e.name) ? [p] : [];
  });
}

function packageOf(spec: string): string {
  const parts = spec.split("/");
  return spec.startsWith("@") ? parts.slice(0, 2).join("/") : parts[0];
}

test("every package a non-test source under pi/ imports is a production dependency", () => {
  const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as { dependencies?: Record<string, string> };
  const deps = pkg.dependencies ?? {};
  const re = /^\s*(?:(?:import|export)\b[^"'`;]*?\bfrom\s*|import\s+)["']([^"']+)["']|\bimport\s*\(\s*["']([^"']+)["']/gm;
  const missing: string[] = [];
  for (const f of tsFiles(piDir)) {
    for (const m of readFileSync(f, "utf8").matchAll(re)) {
      const spec = m[1] ?? m[2];
      if (spec.startsWith(".") || spec.startsWith("/") || spec.startsWith("node:")) continue;
      const name = packageOf(spec);
      if (!(name in deps)) missing.push(`${name} (${relative(piDir, f).split(sep).join("/")})`);
    }
  }
  assert.deepEqual([...new Set(missing)].sort(), []);
});
