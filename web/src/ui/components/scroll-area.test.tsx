// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

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

it("scroll-area wraps its children in a scroll-area-content element", async () => {
  const { mount } = await import("../../testing/render.tsx");
  const { ScrollArea } = await import("./scroll-area.tsx");
  const m = mount(<ScrollArea><i id="kid">k</i></ScrollArea>);
  const content = m.container.querySelector("[data-slot=scroll-area-viewport] [data-slot=scroll-area-content]");
  expect(content).not.toBeNull();
  expect(content?.querySelector("#kid")).not.toBeNull();
  m.unmount();
});

it("scroll-area declares a horizontal scrollbar", () => {
  const source = read("./scroll-area.tsx");
  expect(source).toMatch(/<ScrollBar\s+orientation="horizontal"/);
});
