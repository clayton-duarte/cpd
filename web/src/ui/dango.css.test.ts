// @vitest-environment node
import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");
const css = read("./app.css");
const lower = css.toLowerCase();

it("app css imports tailwind and declares the dango surface tokens", () => {
  const first = css.replace(/\/\*[\s\S]*?\*\//g, "").split("\n").find((l) => l.trim() !== "");
  expect(first?.trim()).toBe('@import "tailwindcss";');
  const hex = [
    "#19171c", "#1c1a20", "#211f26", "#26232c", "#2b2731", "#2e2a33", "#35313b", "#3a3640", "#24212a",
    "#f3ead9", "#d8d2cc", "#aba3af", "#87808c", "#5a504a", "#a1c6ef", "#ecaec7",
  ];
  for (const h of hex) expect(lower, h).toContain(h);
  for (const v of ["--surface-sunken", "--background", "--card", "--surface-raised", "--surface-pressed", "--border", "--input", "--edge", "--row-hover", "--fg-strong", "--foreground", "--muted-foreground", "--fg-dim", "--line", "--ring"]) {
    expect(css, v).toContain(`${v}:`);
  }
  expect(read("../main.tsx")).toContain('import "./ui/app.css";');
});

it("app css declares the glyph classes and the typeset variables", () => {
  for (const g of ["glyph", "glyph-blocked", "glyph-landing", "glyph-decision", "glyph-draft", "glyph-active", "glyph-focus", "glyph-done", "glyph-unread", "glyph-queued", "glyph-idle"]) {
    expect(css, g).toContain(`@utility ${g} {`);
  }
  expect(css).toContain("@keyframes dango-spin");
  expect(css).toMatch(/--typeset-size:\s*1em/);
  expect(css).toMatch(/--typeset-leading:\s*1\.45/);
  expect(css).toMatch(/--typeset-flow:\s*0\.5em/);
  expect(css).toMatch(/font-variant-ligatures:\s*none/);
  expect(css).toContain('"Maple Mono"');
});

it("vite config registers the tailwind plugin", () => {
  const cfg = read("../../vite.config.ts");
  expect(cfg).toContain('import tailwindcss from "@tailwindcss/vite"');
  expect(cfg).toMatch(/plugins:\s*\[react\(\),\s*tailwindcss\(\),\s*cpdWebPlugin\(\)\]/);
});
