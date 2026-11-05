// @vitest-environment node
import { existsSync, readFileSync } from "node:fs";
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
});

it("app css bundles Roboto and Roboto Mono variable fonts", () => {
  expect(css.match(/@font-face\s*\{/g)).toHaveLength(4);
  expect(css).toContain('font-family: "Roboto";');
  expect(css).toContain('font-family: "Roboto Mono";');
  expect(css.match(/font-weight: 100 900;/g)).toHaveLength(2);
  expect(css.match(/font-weight: 100 700;/g)).toHaveLength(2);
  expect(css.match(/font-style: normal;/g)).toHaveLength(2);
  expect(css.match(/font-style: italic;/g)).toHaveLength(2);
  for (const src of [
    "./fonts/Roboto/Roboto-VariableFont_wdth,wght.ttf",
    "./fonts/Roboto/Roboto-Italic-VariableFont_wdth,wght.ttf",
    "./fonts/Roboto_Mono/RobotoMono-VariableFont_wght.ttf",
    "./fonts/Roboto_Mono/RobotoMono-Italic-VariableFont_wght.ttf",
  ]) expect(css).toContain(`url("${src}") format("truetype")`);
  expect(css.match(/font-display: block;/g)).toHaveLength(4);
  expect(css).toContain('--font-sans: "Roboto", sans-serif;');
  expect(css).toContain('--font-mono: "Roboto Mono", monospace;');
  expect(css).toContain("--font-display: var(--font-sans);");
  expect(css).toContain("--font-heading: var(--font-sans);");
  const urls = [...css.matchAll(/url\("(\.\/fonts\/[^"]+)"\)/g)].map((m) => m[1] as string);
  expect(urls).toHaveLength(4);
  for (const u of urls) expect(existsSync(new URL(u, import.meta.url))).toBe(true);
});

it("vite config registers the tailwind plugin", () => {
  const cfg = read("../../vite.config.ts");
  expect(cfg).toContain('import tailwindcss from "@tailwindcss/vite"');
  expect(cfg).toMatch(/plugins:\s*\[react\(\),\s*tailwindcss\(\),\s*cpdWebPlugin\(\)\]/);
});
