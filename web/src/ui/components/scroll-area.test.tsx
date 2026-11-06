import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (relative: string): string => readFileSync(new URL(relative, import.meta.url), "utf8");

it("scroll-area wrapper imports the base-ui scroll-area and not radix-ui", () => {
  const source = read("./scroll-area.tsx");
  expect(source).toContain("@base-ui/react/scroll-area");
  expect(source).not.toContain("radix-ui");
});

it("web package.json pins @base-ui/react to an exact version", () => {
  const pkg = JSON.parse(read("../../../package.json")) as { dependencies: Record<string, string> };
  expect(pkg.dependencies["@base-ui/react"]).toMatch(/^\d+\.\d+\.\d+$/);
});

it("pnpm-lock.yaml locks @base-ui/react", () => {
  expect(read("../../../../pnpm-lock.yaml")).toMatch(/@base-ui\/react@\d+\.\d+\.\d+/);
});
