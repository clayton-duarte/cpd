// @vitest-environment node
import { readFileSync } from "node:fs";

test("the main build emits server.js next to main.js", () => {
  const src = readFileSync(new URL("../../vite.electron.config.ts", import.meta.url), "utf8");
  expect(src).toMatch(/entryFileNames\s*:\s*["']main\.js["']/);
  expect(src).toMatch(/server\.js/);
  expect(src).toMatch(/input\s*:\s*(\{[^}]*server[^}]*\}|\[[^\]]*server[^\]]*\])/);
});
