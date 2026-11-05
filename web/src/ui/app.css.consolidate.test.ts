// @vitest-environment node
import { existsSync, readFileSync } from "node:fs";
import { expect, it } from "vitest";

const css = readFileSync(new URL("./app.css", import.meta.url), "utf8");
const sharedUrl = new URL("../lib/shadcn-tailwind.css", import.meta.url);

it("app.css defines the prompt-border-pulse keyframes", () => {
  const frames = /@keyframes\s+prompt-border-pulse\s*\{([\s\S]*?)\n\}/.exec(css);
  expect(frames, "@keyframes prompt-border-pulse").not.toBeNull();
  expect(frames?.[1], "animates border-color").toContain("border-color");
  expect(frames?.[1], "animates box-shadow").toContain("box-shadow");
  expect(css, "a class applies the animation").toMatch(/\.prompt-border-pulse\s*\{[^}]*animation:\s*prompt-border-pulse/);
});

it("app.css imports shadcn-tailwind.css", () => {
  expect(css).toMatch(/@import\s+"\.\.\/lib\/shadcn-tailwind\.css";/);
});

it("shadcn-tailwind.css lives under web/src/lib", () => {
  expect(existsSync(sharedUrl), "web/src/lib/shadcn-tailwind.css exists").toBe(true);
  const text = readFileSync(sharedUrl, "utf8");
  expect(text, "defines the data-open custom variant").toContain("@custom-variant data-open");
});
