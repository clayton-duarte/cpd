// @vitest-environment node
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";

// The Base UI migration replaces Radix. This test pins the end state: nothing under
// web/src may import the `radix-ui` package (or an `@radix-ui/*` scoped package),
// and web/package.json may not list either as a dependency. It scans source text
// with node fs only; no network.
const here = dirname(fileURLToPath(import.meta.url)); // web/src/ui
const srcRoot = join(here, ".."); // web/src
const webRoot = join(srcRoot, ".."); // web

// Every regular file under `dir`, recursively, in a stable order.
const walk = (dir: string): string[] => {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full));
    else if (entry.isFile()) out.push(full);
  }
  return out;
};

// The module specifier of an `import`/`export ... from`/`require`/dynamic `import`.
const SPECIFIER = /(?:\bfrom\s*|\bimport\s*\(?\s*|\brequire\s*\(\s*)["']([^"']+)["']/g;

const isRadix = (specifier: string): boolean =>
  specifier === "radix-ui" || specifier.startsWith("radix-ui/") || specifier.startsWith("@radix-ui/");

const importsRadix = (file: string): boolean => {
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(SPECIFIER)) {
    const specifier = match[1];
    if (specifier !== undefined && isRadix(specifier)) return true;
  }
  return false;
};

it("no file under web/src imports radix-ui", () => {
  const offenders = walk(srcRoot).filter(importsRadix).map((file) => relative(srcRoot, file));
  expect(offenders, `files importing radix-ui:\n${offenders.join("\n")}`).toEqual([]);
});

it("radix-ui is not a dependency", () => {
  const pkg = JSON.parse(readFileSync(join(webRoot, "package.json"), "utf8")) as {
    dependencies?: Record<string, string>;
    devDependencies?: Record<string, string>;
  };
  const keys = [...Object.keys(pkg.dependencies ?? {}), ...Object.keys(pkg.devDependencies ?? {})];
  const radix = keys.filter((key) => key === "radix-ui" || key.startsWith("@radix-ui/"));
  expect(radix, `radix-ui dependency keys: ${radix.join(", ")}`).toEqual([]);
});
