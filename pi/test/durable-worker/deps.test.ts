import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { test } from "node:test";

const ROOT = resolve(import.meta.dirname, "../../..");
const ENTRY = resolve(ROOT, "pi/durable-worker/main.ts");

/** The specifiers of the run-time imports of a source: every `import ... from`, `export ... from` and bare `import "x"` that is not `import type`. */
function runtimeSpecifiers(source: string): string[] {
  const specifiers: string[] = [];
  for (const m of source.matchAll(/^(?:import|export)\s+(?!type[\s{])[^;]*?\sfrom\s+["']([^"']+)["']/gm)) specifiers.push(m[1]);
  for (const m of source.matchAll(/^import\s+["']([^"']+)["']/gm)) specifiers.push(m[1]);
  return specifiers;
}

/** `file` and every relative module it imports at run time, followed transitively. */
function modulesFrom(entry: string): Map<string, string[]> {
  const seen = new Map<string, string[]>();
  const todo = [entry];
  for (let file = todo.pop(); file !== undefined; file = todo.pop()) {
    if (seen.has(file)) continue;
    const specifiers = runtimeSpecifiers(readFileSync(file, "utf8"));
    seen.set(file, specifiers);
    for (const s of specifiers) if (s.startsWith("./") || s.startsWith("../")) todo.push(resolve(dirname(file), s));
  }
  return seen;
}

/** The package a bare specifier names: `@scope/name/sub` is `@scope/name`, `name/sub` is `name`; `node:` builtins and relative paths are none. */
function packageOf(specifier: string): string | undefined {
  if (specifier.startsWith(".") || specifier.startsWith("node:")) return undefined;
  const parts = specifier.split("/");
  return specifier.startsWith("@") ? parts.slice(0, 2).join("/") : parts[0];
}

test("every package the durable runner imports at run time is a production dependency", () => {
  const manifest = JSON.parse(readFileSync(resolve(ROOT, "package.json"), "utf8")) as { dependencies?: Record<string, string> };
  const dependencies = Object.keys(manifest.dependencies ?? {});
  const modules = modulesFrom(ENTRY);
  assert.ok(modules.size > 1, "the walk followed the relative imports");
  const missing: string[] = [];
  for (const [file, specifiers] of modules) {
    for (const s of specifiers) {
      const name = packageOf(s);
      if (name !== undefined && !dependencies.includes(name)) missing.push(`${file.slice(ROOT.length + 1)} imports ${name}`);
    }
  }
  assert.deepEqual(missing, [], "`pnpm install --prod` leaves out what is not in `dependencies`");
});
