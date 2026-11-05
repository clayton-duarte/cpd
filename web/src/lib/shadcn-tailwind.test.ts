import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

const css = readFileSync(new URL("./shadcn-tailwind.css", import.meta.url), "utf8");

it("shadcn-tailwind.css defines the collapsible-down and collapsible-up keyframes with 200ms and 160ms durations", () => {
  expect(css, "down keyframes").toMatch(/@keyframes collapsible-down\s*\{/);
  expect(css, "up keyframes").toMatch(/@keyframes collapsible-up\s*\{/);
  expect(css, "down utility").toMatch(/@utility animate-collapsible-down\s*\{[^}]*animation:\s*collapsible-down\s+200ms/);
  expect(css, "up utility").toMatch(/@utility animate-collapsible-up\s*\{[^}]*animation:\s*collapsible-up\s+160ms/);
});
