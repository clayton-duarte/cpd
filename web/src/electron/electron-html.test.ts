// @vitest-environment node
import { readFileSync } from "node:fs";

test("electron.html forces the default cursor on the terminal and its canvas", () => {
  const html = readFileSync(new URL("../../electron.html", import.meta.url), "utf8");
  expect(html).toMatch(/#terminal\s*,\s*#terminal\s+canvas\s*\{\s*cursor\s*:\s*default\s*!important\s*;?\s*\}/);
});

test("electron.html and index.html are titled CPD", () => {
  for (const file of ["../../electron.html", "../../index.html"]) {
    const html = readFileSync(new URL(file, import.meta.url), "utf8");
    expect(html).toMatch(/<title>CPD<\/title>/);
  }
});
