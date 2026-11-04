// @vitest-environment node
import { readFileSync } from "node:fs";

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");

test("package.json and electron-builder name the product CPD", () => {
  const pkg = JSON.parse(read("../../package.json")) as { productName?: string };
  expect(pkg.productName).toBe("CPD");
  expect(read("../../electron-builder.yml")).toMatch(/^productName:\s*CPD\b/m);
});

test("the electron script patches the bundle name with plutil on darwin", () => {
  const pkg = JSON.parse(read("../../package.json")) as { scripts: Record<string, string> };
  const script = pkg.scripts["electron"] ?? "";
  expect(script).toContain("plutil");
  expect(script).toContain("CFBundleName");
  expect(script).toContain("CFBundleDisplayName");
  expect(script).toMatch(/darwin/);
});
