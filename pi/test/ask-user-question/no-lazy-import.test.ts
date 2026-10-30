import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { test } from "node:test";
import { stripTypeScriptTypes } from "node:module";

const PI = join(import.meta.dirname, "..", "..");

/** Node's type eraser blanks type positions (`type T = import("./x.ts").U`) and keeps positions; it prints an ExperimentalWarning, which is muted here. */
function erase(source: string): string {
  const emit = process.emitWarning;
  process.emitWarning = (() => {}) as typeof process.emitWarning;
  try {
    return stripTypeScriptTypes(source, { mode: "strip" });
  } finally {
    process.emitWarning = emit;
  }
}

/** `file:line` of every value `import("./...")` call in `source`: not a comment, string or type, not a computed or bare specifier. */
function lazyRelativeImports(file: string, source: string): string[] {
  const code = erase(source);
  const found: string[] = [];
  const braces: number[] = []; // depth of each open template substitution `${`
  let depth = 0;
  let line = 1;
  const skipQuoted = (i: number, q: string): number => {
    for (i++; i < code.length && code[i] !== q; i++) {
      if (code[i] === "\\") i++;
      else if (code[i] === "\n") line++;
    }
    return i;
  };
  // Inside a template's text from `i`; returns the index of its closing backtick or of the `${` that opens a substitution.
  const templateText = (i: number): number => {
    for (; i < code.length; i++) {
      if (code[i] === "\\") i++;
      else if (code[i] === "\n") line++;
      else if (code[i] === "`") return i;
      else if (code[i] === "$" && code[i + 1] === "{") return i;
    }
    return i;
  };
  for (let i = 0; i < code.length; i++) {
    const c = code[i]!;
    if (c === "\n") line++;
    else if (c === "/" && code[i + 1] === "/") while (i < code.length && code[i + 1] !== "\n") i++;
    else if (c === "/" && code[i + 1] === "*") {
      for (i += 2; i < code.length && !(code[i] === "*" && code[i + 1] === "/"); i++) if (code[i] === "\n") line++;
      i++;
    } else if (c === '"' || c === "'") i = skipQuoted(i, c);
    else if (c === "`" || (c === "}" && braces.at(-1) === depth)) {
      if (c === "}") braces.pop();
      i = templateText(i + 1);
      if (code[i] === "$") {
        braces.push(++depth);
        i++;
      }
    } else if (c === "{") depth++;
    else if (c === "}") depth--;
    else if (code.startsWith("import", i) && !/[\w$.]/.test(code[i - 1] ?? " ") && !/[\w$]/.test(code[i + 6] ?? " ")) {
      const m = /^import\s*\(\s*(["'`])\./.exec(code.slice(i, i + 200));
      if (m) found.push(`${file}:${line}`);
    }
  }
  return found;
}

const sources = (dir: string): string[] =>
  readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return e.name === "node_modules" || p === join(PI, "test") ? [] : sources(p);
    return e.name.endsWith(".ts") && !e.name.endsWith(".test.ts") ? [p] : [];
  });

test("no source of the pi adapter imports a relative module lazily", () => {
  assert.deepEqual(lazyRelativeImports("a.ts", 'async function f() { await import("./x.ts"); }'), ["a.ts:1"], "the scanner flags a lazy relative import");
  assert.deepEqual(lazyRelativeImports("b.ts", 'type T = import("./x.ts").U;'), [], "the scanner ignores an import type");
  assert.deepEqual(lazyRelativeImports("d.ts", '// import("./x.ts")\nconst s = "import(\'./x.ts\')"; const t = `import("./x.ts") ${1}`;'), [], "the scanner ignores comments and strings");
  assert.deepEqual(lazyRelativeImports("c.ts", "async function f(url: URL) { await import(url.href); }"), [], "the scanner ignores a computed specifier");
  const lazy = sources(PI).flatMap((f) => lazyRelativeImports(relative(PI, f), readFileSync(f, "utf8")));
  assert.deepEqual(lazy, [], `lazy relative imports:\n${lazy.join("\n")}`);
});
